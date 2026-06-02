// Dump the generated type tree for one MonoBehaviour as `<level> <type> <name> <metaflag>`.
// usage: dump <managed-dir> <unity-version> <assembly.dll> <Namespace.Class>
use unity_typetree_gen::{AssemblyTypeTreeGenerator, TypeTreeNode};

fn print_tree(node: &TypeTreeNode) {
    println!(
        "{} {} {} {}",
        node.m_Level,
        node.m_Type,
        node.m_Name,
        node.m_MetaFlag.unwrap_or(0)
    );
    for child in &node.children {
        print_tree(child);
    }
}

fn main() {
    let args: Vec<String> = std::env::args().collect();
    let (managed_dir, version, assembly, full_name) = (&args[1], &args[2], &args[3], &args[4]);

    let version = version
        .parse()
        .expect("invalid unity version (e.g. 6000.0.0f1)");
    // Resolve assemblies lazily from the Managed directory: only the DLLs
    // actually referenced by the requested MonoBehaviour are read and parsed.
    let managed_dir = std::path::PathBuf::from(managed_dir);
    let generator = AssemblyTypeTreeGenerator::new(version);

    let (namespace, type_name) = full_name.rsplit_once('.').unwrap_or(("", full_name));
    let tree = generator
        .generate_from_dir(&managed_dir, assembly, namespace, type_name)
        .expect("load error")
        .expect("assembly/class not found");
    print_tree(&tree);
}
