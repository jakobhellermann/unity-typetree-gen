// TODO(ai-review): review for style and correctness
//! Compares the generator output for every fixture MonoBehaviour against the
//! committed AssetsTools.NET reference snapshots, for every Unity version under
//! `tests/snapshots/<version>/`.
//!
//! Regenerate the inputs with `just regen` (rebuilds Fixtures.dll and the
//! snapshots); both are committed so `cargo test` needs no .NET toolchain.
use std::collections::BTreeMap;
use std::path::Path;
use std::{cell::RefCell, fs};

use serde::Deserialize;
use unity_typetree_gen::{AssemblyTypeTreeGenerator, TypeTreeNode};

/// A flattened node, matching the snapshot JSON (the AssetsTools.NET reference),
/// so a generated tree can be compared depth-first against it.
#[derive(Debug, PartialEq, Eq)]
#[allow(non_snake_case)]
struct Node {
    m_Type: String,
    m_Name: String,
    m_Level: u8,
    m_MetaFlag: i32,
}

fn flatten(tree: &TypeTreeNode) -> Vec<Node> {
    let mut out = Vec::new();
    fn walk(node: &TypeTreeNode, level: u8, out: &mut Vec<Node>) {
        out.push(Node {
            m_Type: node.m_Type.clone(),
            m_Name: node.m_Name.clone(),
            m_Level: level,
            m_MetaFlag: node.m_MetaFlag.unwrap_or(0),
        });
        for child in &node.children {
            walk(child, level + 1, out);
        }
    }
    walk(tree, 0, &mut out);
    out
}

const FIXTURES_DLL: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/tests/Fixtures.dll");
const UNITY_DLL: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/tests/UnityEngine.dll");
const UNITY_CORE_DLL: &str = concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/tests/UnityEngine.CoreModule.dll"
);
const SNAPSHOTS: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/tests/snapshots");

/// Unity versions snapshotted under `tests/snapshots/<version>/`; kept in sync
/// with `unity_versions` in the justfile.
const VERSIONS: &[&str] = &["6000.0.0f1", "2019.4.0f1", "5.0.0f1"];

const FIXTURES: &[&str] = &[
    "Fixtures.Primitives",
    "Fixtures.Enums",
    "Fixtures.Strings",
    "Fixtures.NestedEnumField",
    "Fixtures.Pointers",
    "Fixtures.SpecialTypes",
    "Fixtures.Arrays",
    "Fixtures.MultidimArrays",
    "Fixtures.Lists",
    "Fixtures.NestedSerializable",
    "Fixtures.FieldVisibility",
    "Fixtures.Inheritance",
    "Fixtures.ManagedRefs",
    "Fixtures.ScriptableRefs",
    "Fixtures.Generics",
    "Fixtures.GenericCollectionArg",
    "Fixtures.GenericInheritance",
    "Fixtures.TwoParamGeneric",
    "Fixtures.NestedTypes",
    // Defined in UnityEngine.CoreModule, forwarded by the UnityEngine.dll facade;
    // covered by `type_forwarder`.
    "UnityEngine.ForwardedAsset",
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

fn new_generator(version: &str) -> AssemblyTypeTreeGenerator {
    AssemblyTypeTreeGenerator::new(version.parse().unwrap())
}

/// Loader over the fixture assemblies. The UnityEngine stubs are split into a
/// facade (`UnityEngine.dll`, only `[TypeForwardedTo]` entries) and the module
/// that holds the real definitions (`UnityEngine.CoreModule.dll`) — mirroring a
/// shipped game and exercising cross-assembly + type-forwarder resolution.
fn fixture_loader(name: &str) -> Result<Vec<u8>, std::io::Error> {
    let path = match name {
        "Fixtures.dll" => FIXTURES_DLL,
        "UnityEngine.dll" => UNITY_DLL,
        "UnityEngine.CoreModule.dll" => UNITY_CORE_DLL,
        _ => return Err(std::io::Error::from(std::io::ErrorKind::NotFound)),
    };
    Ok(read_dll(path))
}

fn check(full_name: &str) {
    check_in("Fixtures.dll", full_name);
}

/// Like [`check`], but generates the type via `assembly` — used for types whose
/// MonoScript names a facade assembly that only forwards them.
fn check_in(assembly: &str, full_name: &str) {
    let (namespace, type_name) = full_name.rsplit_once('.').unwrap_or(("", full_name));

    for version in VERSIONS {
        let generator = new_generator(version);
        let got = generator
            .generate(&fixture_loader, assembly, namespace, type_name)
            .expect("load error")
            .unwrap_or_else(|| panic!("generate {full_name} @ {version}"));
        let want = load_snapshot(version, full_name);
        assert_eq!(
            flatten(&got),
            want,
            "type tree mismatch for {full_name} @ {version}"
        );
    }
}

/// `ForwardedAsset` is defined in `UnityEngine.CoreModule` but a MonoScript names
/// it under the `UnityEngine.dll` facade, which only forwards it. Generating via
/// the facade must follow the forwarder to the module and produce the full type
/// tree (regression: the top-level lookup previously didn't follow forwarders
/// and returned an empty type).
#[test]
fn type_forwarder() {
    check_in("UnityEngine.dll", "UnityEngine.ForwardedAsset");
}

/// A type that doesn't exist in the assembly (e.g. a MonoScript naming an
/// editor-only or version-mismatched type) must report not-found (`None`),
/// not a bogus header-only tree — distinct from a real type with no serialized
/// fields, which yields `Some` with just the `Base` node.
#[test]
fn missing_type_is_none() {
    let generator = new_generator(VERSIONS[0]);
    assert_eq!(
        generator
            .generate(&fixture_loader, "Fixtures.dll", "Fixtures", "NoSuchType")
            .expect("load error"),
        None,
    );
    // A real, field-less MonoBehaviour still resolves (Some, just the Base node).
    let empty = generator
        .generate(
            &fixture_loader,
            "UnityEngine.CoreModule.dll",
            "UnityEngine",
            "MonoBehaviour",
        )
        .expect("load error")
        .expect("MonoBehaviour resolves");
    assert!(
        empty.children.is_empty(),
        "expected only the Base node, got {empty:?}"
    );
}

/// `Inner` exists only as a *nested* type (`Fixtures.Outer/Inner`), not as a
/// top-level type. A top-level lookup (as a MonoScript with no namespace records
/// it) must not spuriously match the nested type — it should report not-found.
#[test]
fn nested_type_not_matched_as_top_level() {
    let generator = new_generator(VERSIONS[0]);
    assert_eq!(
        generator
            .generate(&fixture_loader, "Fixtures.dll", "", "Inner")
            .expect("load error"),
        None
    );
}

/// A loader resolves assembly bytes lazily by name, and only for the assemblies
/// actually touched: generating a `Fixtures.dll` type that references no other
/// assembly must not load the UnityEngine stubs.
#[test]
fn lazy_loader_only_loads_what_is_used() {
    let loaded = RefCell::new(Vec::new());
    let loader = |name: &str| -> Result<Vec<u8>, std::io::Error> {
        loaded.borrow_mut().push(name.to_string());
        fixture_loader(name)
    };
    let generator = AssemblyTypeTreeGenerator::new(VERSIONS[0].parse().unwrap());

    // Primitives reference only built-in types, so only Fixtures.dll loads.
    let got = generator
        .generate(&loader, "Fixtures.dll", "Fixtures", "Primitives")
        .expect("load error")
        .expect("generate Primitives");
    assert_eq!(
        flatten(&got),
        load_snapshot(VERSIONS[0], "Fixtures.Primitives")
    );
    assert_eq!(*loaded.borrow(), vec!["Fixtures.dll".to_string()]);

    // A second lookup in the same assembly does not re-invoke the loader.
    generator
        .generate(&loader, "Fixtures.dll", "Fixtures", "Enums")
        .expect("load error")
        .expect("generate Enums");
    assert_eq!(*loaded.borrow(), vec!["Fixtures.dll".to_string()]);
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
    nested_enum_field => "Fixtures.NestedEnumField",
    pointers => "Fixtures.Pointers",
    special_types => "Fixtures.SpecialTypes",
    arrays => "Fixtures.Arrays",
    multidim_arrays => "Fixtures.MultidimArrays",
    lists => "Fixtures.Lists",
    nested_serializable => "Fixtures.NestedSerializable",
    field_visibility => "Fixtures.FieldVisibility",
    inheritance => "Fixtures.Inheritance",
    managed_refs => "Fixtures.ManagedRefs",
    scriptable_refs => "Fixtures.ScriptableRefs",
    generics => "Fixtures.Generics",
    generic_collection_arg => "Fixtures.GenericCollectionArg",
    generic_inheritance => "Fixtures.GenericInheritance",
    two_param_generic => "Fixtures.TwoParamGeneric",
    nested_types => "Fixtures.NestedTypes",
}

// --- monobehaviour_definitions tests ---

fn def_names(defs: &BTreeMap<String, Vec<String>>) -> std::collections::HashSet<&str> {
    defs.values().flatten().map(|s| s.as_str()).collect()
}

/// Cross-check Rust `monobehaviour_definitions` against the C#-generated snapshot list.
///
/// `snapshot-gen/Program.cs` (AssetsTools.NET + Mono.Cecil `IsMonoBehaviour`) wrote
/// one `.json` file per MonoBehaviour it found when `just regen` was last run.
/// The set of snapshot filenames is therefore the canonical C# answer to
/// "which types are MonoBehaviours?".  Loading the same fixture assemblies and
/// comparing the full-name lists checks that the Rust implementation agrees with
/// the C# implementation exactly — including transitive cases like
/// `Inheritance : Primitives : MonoBehaviour` and excluding `ScriptableObject`.
#[test]
fn monobehaviour_definitions_matches_csharp_snapshots() {
    let generator = new_generator(VERSIONS[0]);
    for asm in &[
        "Fixtures.dll",
        "UnityEngine.dll",
        "UnityEngine.CoreModule.dll",
    ] {
        generator
            .load_assembly(&fixture_loader, asm)
            .unwrap_or_else(|e| panic!("load {asm}: {e}"));
    }

    let defs = generator
        .monobehaviour_definitions(&fixture_loader)
        .expect("monobehaviour_definitions");

    let mut rust_names: Vec<String> = defs.into_values().flatten().collect();
    rust_names.sort();

    // Read the snapshot filenames — these are what C# determined to be MonoBehaviours.
    let dir = format!("{SNAPSHOTS}/{}", VERSIONS[0]);
    let mut cs_names: Vec<String> = fs::read_dir(Path::new(&dir))
        .unwrap_or_else(|e| panic!("read {dir}: {e} (run `just regen`)"))
        .filter_map(|e| e.ok())
        .filter_map(|e| {
            e.file_name()
                .to_string_lossy()
                .strip_suffix(".json")
                .map(str::to_string)
        })
        .collect();
    cs_names.sort();

    assert_eq!(rust_names, cs_names);
}

/// `Outer.NestedMonoBehaviour` is a MonoBehaviour nested inside `Outer` (a plain
/// serializable class). It must NOT appear in `monobehaviour_definitions` because
/// Mono.Cecil's `Types` only yields top-level types, not nested ones.
#[test]
fn monobehaviour_definitions_excludes_nested_monobehaviours() {
    let generator = new_generator(VERSIONS[0]);
    generator
        .load_assembly(&fixture_loader, "Fixtures.dll")
        .expect("load Fixtures.dll");
    generator
        .load_assembly(&fixture_loader, "UnityEngine.CoreModule.dll")
        .expect("load CoreModule");

    let defs = generator
        .monobehaviour_definitions(&fixture_loader)
        .expect("monobehaviour_definitions");

    let names = def_names(&defs);
    // Nested types have no namespace in .NET metadata, so the name has no prefix.
    assert!(
        !names.contains("NestedMonoBehaviour"),
        "nested MonoBehaviour must not be returned"
    );
}

/// MonoBehaviour itself and all its ancestors (Behaviour, Component, Object)
/// must not appear, even when CoreModule is loaded. Only types that *derive from*
/// MonoBehaviour (not MonoBehaviour itself) are returned.
///
/// Also verifies that `ForwardedAsset : MonoBehaviour` (defined in CoreModule)
/// is included, and that ScriptableObject is excluded even though it shares
/// `UnityEngine.Object` as a common ancestor with MonoBehaviour.
#[test]
fn monobehaviour_definitions_excludes_base_classes() {
    let generator = new_generator(VERSIONS[0]);
    generator
        .load_assembly(&fixture_loader, "UnityEngine.CoreModule.dll")
        .expect("load CoreModule");

    let defs = generator
        .monobehaviour_definitions(&fixture_loader)
        .expect("monobehaviour_definitions");

    let names = def_names(&defs);

    // ForwardedAsset : MonoBehaviour is a real MonoBehaviour and must be included.
    assert!(
        names.contains("UnityEngine.ForwardedAsset"),
        "ForwardedAsset (MonoBehaviour subclass in CoreModule) must be included"
    );

    // MonoBehaviour itself does not *derive from* MonoBehaviour.
    assert!(
        !names.contains("UnityEngine.MonoBehaviour"),
        "MonoBehaviour itself must not be returned"
    );

    // Ancestors of MonoBehaviour — all reachable via BASE_STOP short-circuit.
    assert!(!names.contains("UnityEngine.Behaviour"));
    assert!(!names.contains("UnityEngine.Component"));
    assert!(!names.contains("UnityEngine.Object"));

    // ScriptableObject shares the UnityEngine.Object ancestor but is not a
    // MonoBehaviour; BASE_STOP correctly terminates the walk at ScriptableObject.
    assert!(!names.contains("UnityEngine.ScriptableObject"));
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
