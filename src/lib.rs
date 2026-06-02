// TODO(ai-review): review for style and correctness
use dotnetdll::prelude::Resolution;

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
///
/// TODO: port AssetsTools.NET's `MonoCecilTempGenerator`. This is currently a
/// stub that emits only the `Base` root, so the snapshot tests run red.
pub fn generate(
    resolution: &Resolution,
    namespace: &str,
    type_name: &str,
    unity_version: &str,
) -> Vec<Node> {
    let _ = (resolution, namespace, unity_version);
    vec![Node {
        m_Type: type_name.to_string(),
        m_Name: "Base".to_string(),
        m_Level: 0,
        m_MetaFlag: 0,
    }]
}
