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
    let mut generator = AssemblyTypeTreeGenerator::new(version);
    for entry in std::fs::read_dir(managed_dir).unwrap() {
        let path = entry.unwrap().path();
        if path.extension().is_some_and(|e| e == "dll") {
            let name = path.file_name().unwrap().to_str().unwrap().to_string();
            generator.add_assembly(name, std::fs::read(&path).unwrap());
        }
    }

    let (namespace, type_name) = full_name.rsplit_once('.').unwrap_or(("", full_name));
    let tree = generator
        .generate(assembly, namespace, type_name)
        .expect("assembly/class not found");
    print_tree(&tree);
}
