// TODO(ai-review): review for style and correctness
//! Compares the generator output for every fixture MonoBehaviour against the
//! committed AssetsTools.NET reference snapshot (`tests/snapshots/*.json`).
//!
//! Regenerate the inputs with `just regen` (rebuilds Fixtures.dll and the
//! snapshots); both are committed so `cargo test` needs no .NET toolchain.
use std::fs;
use std::path::Path;

use dotnetdll::prelude::{ReadOptions, Resolution};
use serde::Deserialize;
use unity_typetree_gen::{ALIGN_FLAG, Node, generate};

const DLL: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/tests/Fixtures.dll");
const SNAPSHOTS: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/tests/snapshots");
const UNITY_VERSION: &str = "6000.0.0";

#[derive(Debug, Deserialize, PartialEq, Eq)]
#[allow(non_snake_case)]
struct SnapshotNode {
    m_Type: String,
    m_Name: String,
    m_Level: u8,
    m_MetaFlag: i32,
}

fn load_snapshot(full_name: &str) -> Vec<Node> {
    let path = format!("{SNAPSHOTS}/{full_name}.json");
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

fn check(full_name: &str) {
    let bytes = fs::read(DLL).unwrap_or_else(|e| panic!("read {DLL}: {e} (run `just regen`)"));
    let resolution = Resolution::parse(&bytes, ReadOptions::default()).expect("parse Fixtures.dll");
    let (namespace, type_name) = full_name.rsplit_once('.').unwrap_or(("", full_name));

    let got = generate(&resolution, namespace, type_name, UNITY_VERSION);
    let want = load_snapshot(full_name);
    assert_eq!(got, want, "type tree mismatch for {full_name}");
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
}

/// Every committed snapshot must have a corresponding test above, so adding a
/// fixture without wiring it up fails loudly rather than silently skipping.
#[test]
fn all_snapshots_are_covered() {
    let covered = [
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
    ];
    let mut on_disk: Vec<String> = fs::read_dir(Path::new(SNAPSHOTS))
        .unwrap_or_else(|e| panic!("read {SNAPSHOTS}: {e} (run `just regen`)"))
        .filter_map(|e| e.ok())
        .filter_map(|e| {
            let name = e.file_name().to_string_lossy().into_owned();
            name.strip_suffix(".json").map(str::to_string)
        })
        .collect();
    on_disk.sort();
    let mut expected: Vec<String> = covered.iter().map(|s| s.to_string()).collect();
    expected.sort();
    assert_eq!(on_disk, expected, "snapshots on disk vs. covered tests");
    let _ = ALIGN_FLAG;
}
