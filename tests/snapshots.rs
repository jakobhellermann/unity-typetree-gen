// TODO(ai-review): review for style and correctness
//! Compares the generator output for every fixture MonoBehaviour against the
//! committed AssetsTools.NET reference snapshots, for every Unity version under
//! `tests/snapshots/<version>/`.
//!
//! Regenerate the inputs with `just regen` (rebuilds Fixtures.dll and the
//! snapshots); both are committed so `cargo test` needs no .NET toolchain.
use std::fs;
use std::path::Path;

use serde::Deserialize;
use unity_typetree_gen::{AssemblyTypeTreeGenerator, Node};

const FIXTURES_DLL: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/tests/Fixtures.dll");
const UNITY_DLL: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/tests/UnityEngine.dll");
const SNAPSHOTS: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/tests/snapshots");

/// Unity versions snapshotted under `tests/snapshots/<version>/`; kept in sync
/// with `unity_versions` in the justfile.
const VERSIONS: &[&str] = &["6000.0.0", "2019.4.0", "5.0.0"];

const FIXTURES: &[&str] = &[
    "Fixtures.Primitives",
    "Fixtures.Enums",
    "Fixtures.Strings",
    "Fixtures.Pointers",
    "Fixtures.SpecialTypes",
    "Fixtures.Arrays",
    "Fixtures.Lists",
    "Fixtures.NestedSerializable",
    "Fixtures.FieldVisibility",
    "Fixtures.Inheritance",
    "Fixtures.ManagedRefs",
    "Fixtures.ScriptableRefs",
    "Fixtures.Generics",
    "Fixtures.GenericCollectionArg",
    "Fixtures.GenericInheritance",
    "Fixtures.NestedTypes",
];

#[derive(Debug, Deserialize, PartialEq, Eq)]
#[allow(non_snake_case)]
struct SnapshotNode {
    m_Type: String,
    m_Name: String,
    m_Level: u8,
    m_MetaFlag: i32,
}

fn load_snapshot(version: &str, full_name: &str) -> Vec<Node> {
    let path = format!("{SNAPSHOTS}/{version}/{full_name}.json");
    let json = fs::read_to_string(&path)
        .unwrap_or_else(|e| panic!("read snapshot {path}: {e} (run `just regen`)"));
    let nodes: Vec<SnapshotNode> = serde_json::from_str(&json).unwrap();
    nodes
        .into_iter()
        .map(|n| Node {
            m_Type: n.m_Type,
            m_Name: n.m_Name,
            m_Level: n.m_Level,
            m_MetaFlag: n.m_MetaFlag,
        })
        .collect()
}

fn read_dll(path: &str) -> Vec<u8> {
    fs::read(path).unwrap_or_else(|e| panic!("read {path}: {e} (run `just regen`)"))
}

fn check(full_name: &str) {
    let (namespace, type_name) = full_name.rsplit_once('.').unwrap_or(("", full_name));

    for version in VERSIONS {
        // The UnityEngine stubs are a separate assembly, so its types are
        // cross-assembly references the generator must resolve.
        let mut generator = AssemblyTypeTreeGenerator::new(*version);
        generator.add_assembly("Fixtures.dll", read_dll(FIXTURES_DLL));
        generator.add_assembly("UnityEngine.dll", read_dll(UNITY_DLL));

        let got = generator
            .generate("Fixtures.dll", namespace, type_name)
            .unwrap_or_else(|| panic!("generate {full_name} @ {version}"));
        let want = load_snapshot(version, full_name);
        assert_eq!(got, want, "type tree mismatch for {full_name} @ {version}");
    }
}

macro_rules! snapshot_tests {
    ($($name:ident => $full:literal),+ $(,)?) => {
        $(#[test] fn $name() { check($full); })+
    };
}

snapshot_tests! {
    primitives => "Fixtures.Primitives",
    enums => "Fixtures.Enums",
    strings => "Fixtures.Strings",
    pointers => "Fixtures.Pointers",
    special_types => "Fixtures.SpecialTypes",
    arrays => "Fixtures.Arrays",
    lists => "Fixtures.Lists",
    nested_serializable => "Fixtures.NestedSerializable",
    field_visibility => "Fixtures.FieldVisibility",
    inheritance => "Fixtures.Inheritance",
    managed_refs => "Fixtures.ManagedRefs",
    scriptable_refs => "Fixtures.ScriptableRefs",
    generics => "Fixtures.Generics",
    generic_collection_arg => "Fixtures.GenericCollectionArg",
    generic_inheritance => "Fixtures.GenericInheritance",
    nested_types => "Fixtures.NestedTypes",
}

/// Every committed snapshot must have a corresponding fixture test above (per
/// version), so adding a fixture without wiring it up fails loudly.
#[test]
fn all_snapshots_are_covered() {
    let mut expected: Vec<String> = FIXTURES.iter().map(|s| s.to_string()).collect();
    expected.sort();
    for version in VERSIONS {
        let dir = format!("{SNAPSHOTS}/{version}");
        let mut on_disk: Vec<String> = fs::read_dir(Path::new(&dir))
            .unwrap_or_else(|e| panic!("read {dir}: {e} (run `just regen`)"))
            .filter_map(|e| e.ok())
            .filter_map(|e| {
                let name = e.file_name().to_string_lossy().into_owned();
                name.strip_suffix(".json").map(str::to_string)
            })
            .collect();
        on_disk.sort();
        assert_eq!(
            on_disk, expected,
            "snapshots on disk vs. covered tests for {version}"
        );
    }
}
