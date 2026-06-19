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
    let mut root = build_node(&base);
    // Unity always stores the MonoBehaviour/ScriptableObject root as variable-size (-1), even when every field is fixed.
    root.m_ByteSize = -1;
    root
}

fn build_node(field: &TemplateField) -> TypeTreeNode {
    let children: Vec<TypeTreeNode> = field.children.iter().map(build_node).collect();
    let mut node = TypeTreeNode {
        m_Type: field.ty.clone(),
        m_Name: field.name.clone(),
        m_MetaFlag: Some(if field.aligned { ALIGN_FLAG } else { 0 }),
        children,
        ..Default::default()
    };
    node.m_ByteSize = byte_size(&node);
    // Unity aligns an `Array` node iff its element is a scalar or a string (validated against Unity's embedded type
    // trees); arrays of structs/classes/PPtrs are not aligned. Decided here via rabex's `classify()` on the element.
    if node.m_Type == "Array" {
        // m_TypeFlags=1 marks this node as an array; without it Unity won't iterate it (re-reads element 0 for every
        // entry, e.g. a vector<string> comes back as N copies of the first string).
        node.m_TypeFlags = 1;
        node.m_MetaFlag = Some(if array_element_aligned(&node) { ALIGN_FLAG } else { 0 });
    }
    node
}

/// Whether an `Array` node's element (`data` child) is the kind Unity 4-byte aligns: a scalar, a `string`, or a pure
/// POD struct (every field transitively a scalar — e.g. Vector3f/ColorRGBA/Quaternionf/Keyframe). Arrays of structs
/// that hold a PPtr or string (PPtr itself, or custom classes) are NOT aligned. Validated against Unity's embedded trees.
fn array_element_aligned(array_node: &TypeTreeNode) -> bool {
    array_node
        .children
        .iter()
        .find(|c| c.m_Name == "data")
        .is_some_and(|data| matches!(data.classify(), rabex::typetree::TypetreeNodeKind::String) || is_pod_value_type(data))
}

/// A scalar, or a struct (not a PPtr) whose every field is a POD value type AND whose serialized size is 4-aligned.
/// Unity 4-aligns arrays of its built-in value types (Vector3f, ColorRGBA, Keyframe, … — all 4-byte multiples). The
/// size%4 guard is what keeps this SAFE: it never aligns a struct whose array elements aren't already 4-aligned (a
/// custom struct ending in a bool/byte), so we never insert padding the data doesn't have → never desync. Aligning a
/// 4-aligned struct's array is at worst a redundant no-op.
fn is_pod_value_type(node: &TypeTreeNode) -> bool {
    use rabex::typetree::TypetreeNodeKind::*;
    match node.classify() {
        Bool | U8 | U16 | U32 | U64 | I8 | I16 | I32 | I64 | Float | Double | Char => true,
        Struct => {
            !node.m_Type.starts_with("PPtr")
                && node.m_ByteSize > 0
                && node.m_ByteSize % 4 == 0
                && node.children.iter().all(is_pod_value_type)
        }
        _ => false,
    }
}

/// Unity's `m_ByteSize`: the fixed width of a primitive, -1 for dynamic containers (string/Array/vector/map/…) or any
/// struct with a variable-size member, else the sum of the children's sizes (no inter-field padding — alignment is
/// carried by `m_MetaFlag`, not the size). Primitive widths come from rabex's own `classify()`, so every serialized
/// type-name alias (`short`, `long long`, `FileSize`, `Type*`, …) is covered. Matches Unity's embedded type trees.
fn byte_size(node: &TypeTreeNode) -> i32 {
    use rabex::typetree::TypetreeNodeKind::*;
    match node.classify() {
        Bool | U8 | I8 | Char => 1,
        U16 | I16 => 2,
        U32 | I32 | Float => 4,
        U64 | I64 | Double => 8,
        String | Map | Array | Untyped => -1,
        _ => {
            let mut total = 0i32;
            for c in &node.children {
                if c.m_ByteSize < 0 {
                    return -1;
                }
                total += c.m_ByteSize;
            }
            total
        }
    }
}
