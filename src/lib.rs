//! Generate Unity [`TypeTreeNode`] trees for `MonoBehaviour` and `ScriptableObject`
//! scripts directly from a game's .NET assemblies.
//!
//! See [`AssemblyTypeTreeGenerator`] for a usage example.
mod assembly;
mod generator;
mod template;

pub use assembly::{AssemblyTypeTreeGenerator, Loader};
pub use rabex::UnityVersion;
pub use rabex::typetree::TypeTreeNode;

use template::TemplateField;

/// Align flag set on a node whose value is 4-byte aligned after writing.
const ALIGN_FLAG: i32 = 0x4000;

/// Wrap a script's serialized fields in the `Base` root (type = the class name)
/// and build the [`TypeTreeNode`] tree directly.
pub(crate) fn assemble(children: Vec<TemplateField>, type_name: &str) -> TypeTreeNode {
    let base = TemplateField {
        name: "Base".to_string(),
        ty: type_name.to_string(),
        aligned: false,
        children,
    };
    build_node(&base)
}

fn build_node(field: &TemplateField) -> TypeTreeNode {
    TypeTreeNode {
        m_Type: field.ty.clone(),
        m_Name: field.name.clone(),
        m_MetaFlag: Some(if field.aligned { ALIGN_FLAG } else { 0 }),
        children: field.children.iter().map(build_node).collect(),
        ..Default::default()
    }
}
