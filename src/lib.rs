// TODO(ai-review): review for style and correctness
use dotnetdll::prelude::Resolution;

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

/// Generate the flattened type tree for the MonoBehaviour `namespace.type_name`
/// defined in `resolution`, as Unity would embed it for `unity_version`.
///
/// The result starts with the `Base` root (type = the class name, level 0),
/// followed by the serialized fields in depth order.
pub fn generate(
    resolution: &Resolution,
    namespace: &str,
    type_name: &str,
    unity_version: &str,
) -> Vec<Node> {
    let version = UnityVersion::parse(unity_version);
    let children = generator::Generator::read(resolution, namespace, type_name, version);
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
