// TODO(ai-review): review for style and correctness
mod assembly;
mod generator;
mod template;
mod version;

pub use assembly::AssemblyTypeTreeGenerator;
pub use version::UnityVersion;

use template::TemplateField;

/// A flattened Unity type-tree node. Matches the snapshot JSON emitted by
/// `snapshot-gen` (the AssetsTools.NET reference), so the two compare directly.
#[derive(Debug, Clone, PartialEq, Eq)]
#[allow(non_snake_case)]
pub struct Node {
    pub m_Type: String,
    pub m_Name: String,
    pub m_Level: u8,
    pub m_MetaFlag: i32,
}

/// Align flag set on a node whose value is 4-byte aligned after writing.
pub const ALIGN_FLAG: i32 = 0x4000;

/// Wrap a script's serialized fields in the `Base` root (type = the class name,
/// level 0) and flatten the tree to the depth-ordered node list.
pub(crate) fn assemble(children: Vec<TemplateField>, type_name: &str) -> Vec<Node> {
    let base = TemplateField {
        name: "Base".to_string(),
        ty: type_name.to_string(),
        aligned: false,
        children,
    };
    let mut nodes = Vec::new();
    flatten(&base, 0, &mut nodes);
    nodes
}

fn flatten(field: &TemplateField, level: u8, out: &mut Vec<Node>) {
    out.push(Node {
        m_Type: field.ty.clone(),
        m_Name: field.name.clone(),
        m_Level: level,
        m_MetaFlag: if field.aligned { ALIGN_FLAG } else { 0 },
    });
    for child in &field.children {
        flatten(child, level + 1, out);
    }
}
