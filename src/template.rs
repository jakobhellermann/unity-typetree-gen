// TODO(ai-review): review for style and correctness
//! Port of AssetsTools.NET `CommonMonoTemplateHelper`: the hardcoded template
//! subtrees for strings, vectors, PPtrs, managed references and Unity's
//! built-in serialized value types.
//!
//! Each node carries only what the flattened output needs: a type name, a
//! field name, the children, and whether the node is 4-byte aligned. Alignment
//! is set explicitly here (matching the C# `align` arguments); the dynamically
//! generated fields in [`crate::generator`] derive it from the value type via
//! [`type_aligns_by_name`].
use rabex::UnityVersion;

#[derive(Debug, Clone)]
pub(crate) struct TemplateField {
    pub name: String,
    pub ty: String,
    pub aligned: bool,
    pub children: Vec<TemplateField>,
}

fn node(name: &str, ty: &str, aligned: bool, children: Vec<TemplateField>) -> TemplateField {
    TemplateField {
        name: name.to_string(),
        ty: ty.to_string(),
        aligned,
        children,
    }
}

fn leaf(name: &str, ty: &str, aligned: bool) -> TemplateField {
    node(name, ty, aligned, Vec::new())
}

// --- Basic scalars (align defaults to false in the C# helpers) ---

fn sbyte(name: &str, align: bool) -> TemplateField {
    leaf(name, "SInt8", align)
}
fn byte_f(name: &str, align: bool) -> TemplateField {
    leaf(name, "UInt8", align)
}
fn cchar(name: &str) -> TemplateField {
    leaf(name, "char", false)
}
fn ushort(name: &str) -> TemplateField {
    leaf(name, "UInt16", false)
}
fn int_f(name: &str) -> TemplateField {
    leaf(name, "int", false)
}
fn uint_f(name: &str) -> TemplateField {
    leaf(name, "unsigned int", false)
}
fn long_f(name: &str) -> TemplateField {
    leaf(name, "SInt64", false)
}
fn float_f(name: &str) -> TemplateField {
    leaf(name, "float", false)
}

// --- Value-type classification for alignment of dynamic fields ---

/// Whether a node with this AssetsTools type name is 4-byte aligned, i.e. its
/// value type is one of Bool/Int8/UInt8/Int16/UInt16 (`TypeAligns` over
/// `GetValueTypeByTypeName`).
pub(crate) fn type_aligns_by_name(ty: &str) -> bool {
    matches!(
        ty,
        "bool"
            | "SInt8"
            | "char"
            | "UInt8"
            | "unsigned char"
            | "SInt16"
            | "short"
            | "UInt16"
            | "unsigned short"
    )
}

/// `ConvertBaseToPrimitive`: .NET primitive/enum-underlying full name → the
/// AssetsTools type-tree name.
pub(crate) fn convert_base_to_primitive(full_name: &str) -> Option<&'static str> {
    Some(match full_name {
        "System.Boolean" => "UInt8", // official conversion despite `bool` existing
        "System.SByte" => "SInt8",
        "System.Byte" => "UInt8",
        "System.Char" => "UInt16",
        "System.Int16" => "SInt16",
        "System.UInt16" => "UInt16",
        "System.Int32" => "int",
        "System.UInt32" => "unsigned int",
        "System.Int64" => "SInt64",
        "System.UInt64" => "UInt64",
        "System.Double" => "double",
        "System.Single" => "float",
        "System.String" => "string",
        _ => return None,
    })
}

pub(crate) const SPECIAL_UNITY_TYPES: &[&str] = &[
    "UnityEngine.Color",
    "UnityEngine.Color32",
    "UnityEngine.Gradient",
    "UnityEngine.Vector2",
    "UnityEngine.Vector3",
    "UnityEngine.Vector4",
    "UnityEngine.LayerMask",
    "UnityEngine.Quaternion",
    "UnityEngine.Bounds",
    "UnityEngine.Rect",
    "UnityEngine.RectOffset",
    "UnityEngine.Matrix4x4",
    "UnityEngine.AnimationCurve",
    "UnityEngine.GUIStyle",
    "UnityEngine.Vector2Int",
    "UnityEngine.Vector3Int",
    "UnityEngine.PropertyName",
    "UnityEngine.BoundsInt",
];

pub(crate) fn is_special_unity_type(full_name: &str) -> bool {
    SPECIAL_UNITY_TYPES.contains(&full_name)
}

const BLACKLISTED_ASSEMBLIES: &[&str] = &[
    "mscorlib",
    "netstandard",
    "System.Core",
    "System",
    "System.Private.CoreLib",
    "System.Collections",
    "System.Collections.NonGeneric",
];

pub(crate) fn is_assembly_blacklisted(assembly: &str) -> bool {
    let trimmed = assembly.strip_suffix(".dll").unwrap_or(assembly);
    BLACKLISTED_ASSEMBLIES.contains(&trimmed)
}

// --- string / array / vector ---

/// Children of a `string` node: an aligned `Array` of `char data`.
pub(crate) fn string_children() -> Vec<TemplateField> {
    array_of(cchar("data"))
}

/// `Array(field)`: an `Array` node wrapping `size` and a `data` element carrying `field`'s type/children. The Array's
/// alignment is NOT decided here — `build_node` sets it from the element kind via rabex's `classify()` (Unity aligns
/// arrays of scalars/strings, not arrays of structs). The element's own align is dropped.
fn array_of(field: TemplateField) -> Vec<TemplateField> {
    let data = node("data", &field.ty, false, field.children);
    vec![node("Array", "Array", false, vec![int_f("size"), data])]
}

/// `Vector(field)`: a `vector` node whose element is `field`.
pub(crate) fn vector(field: TemplateField) -> TemplateField {
    let name = field.name.clone();
    node(&name, "vector", false, array_of(field))
}

/// `VectorWithType(field)`: like [`vector`] but the node keeps `field`'s type.
pub(crate) fn vector_with_type(field: TemplateField) -> TemplateField {
    let name = field.name.clone();
    let ty = field.ty.clone();
    node(&name, &ty, false, array_of(field))
}

// --- PPtr / managed references ---

pub(crate) fn pptr_children(version: &UnityVersion) -> Vec<TemplateField> {
    if version.major >= 5 {
        vec![int_f("m_FileID"), long_f("m_PathID")]
    } else {
        vec![int_f("m_FileID"), int_f("m_PathID")]
    }
}

pub(crate) fn managed_reference_children(version: &UnityVersion) -> Vec<TemplateField> {
    if version.major > 2021 || (version.major == 2021 && version.minor >= 2) {
        vec![long_f("rid")]
    } else {
        vec![int_f("id")]
    }
}

fn referenced_managed_type(name: &str) -> TemplateField {
    node(
        name,
        "ReferencedManagedType",
        false,
        vec![
            string_field("class"),
            string_field("ns"),
            string_field("asm"),
        ],
    )
}

fn referenced_object(name: &str, version: &UnityVersion) -> TemplateField {
    let children = if version.major > 2021 || (version.major == 2021 && version.minor >= 2) {
        vec![
            long_f("rid"),
            referenced_managed_type("type"),
            node("data", "ReferencedObjectData", false, Vec::new()),
        ]
    } else {
        vec![
            referenced_managed_type("type"),
            node("data", "ReferencedObjectData", false, Vec::new()),
        ]
    };
    node(name, "ReferencedObject", false, children)
}

/// The `references` registry appended once when a type uses `[SerializeReference]`.
pub(crate) fn managed_references_registry(name: &str, version: &UnityVersion) -> TemplateField {
    let children = if version.major > 2021 || (version.major == 2021 && version.minor >= 2) {
        vec![
            int_f("version"),
            vector(referenced_object("RefIds", version)),
        ]
    } else {
        vec![int_f("version"), referenced_object("00000000", version)]
    };
    node(name, "ManagedReferencesRegistry", false, children)
}

fn string_field(name: &str) -> TemplateField {
    node(name, "string", false, string_children())
}

// --- Special Unity value types ---

fn vector3f(name: &str) -> TemplateField {
    node(
        name,
        "Vector3f",
        false,
        vec![float_f("x"), float_f("y"), float_f("z")],
    )
}

fn vector3int(name: &str) -> TemplateField {
    node(
        name,
        "int3_storage",
        false,
        vec![int_f("x"), int_f("y"), int_f("z")],
    )
}

fn rgbaf(name: &str) -> TemplateField {
    node(
        name,
        "ColorRGBA",
        false,
        vec![float_f("r"), float_f("g"), float_f("b"), float_f("a")],
    )
}

fn bool_f(name: &str, align: bool) -> TemplateField {
    leaf(name, "bool", align)
}

fn vector2f(name: &str) -> TemplateField {
    node(name, "Vector2f", false, vec![float_f("x"), float_f("y")])
}

fn rect_offset(name: &str) -> TemplateField {
    node(
        name,
        "RectOffset",
        false,
        vec![
            int_f("m_Left"),
            int_f("m_Right"),
            int_f("m_Top"),
            int_f("m_Bottom"),
        ],
    )
}

fn pptr_field(name: &str, type_name: &str, version: &UnityVersion) -> TemplateField {
    node(
        name,
        &format!("PPtr<{type_name}>"),
        false,
        pptr_children(version),
    )
}

/// `GUIStyleState` = a background PPtr + a text color.
fn gui_style_state(name: &str, version: &UnityVersion) -> TemplateField {
    node(
        name,
        "GUIStyleState",
        false,
        vec![
            pptr_field("m_Background", "Texture2D", version),
            rgbaf("m_TextColor"),
        ],
    )
}

/// `GUIStyle`'s fields, mirroring `CommonMonoTemplateHelper.GUIStyle`. The field
/// order and a few alignment flags differ between Unity 3 and 4+.
fn gui_style_children(version: &UnityVersion) -> Vec<TemplateField> {
    let v4 = version.major >= 4;
    let mut out = vec![
        string_field("m_Name"),
        gui_style_state("m_Normal", version),
        gui_style_state("m_Hover", version),
        gui_style_state("m_Active", version),
        gui_style_state("m_Focused", version),
        gui_style_state("m_OnNormal", version),
        gui_style_state("m_OnHover", version),
        gui_style_state("m_OnActive", version),
        gui_style_state("m_OnFocused", version),
        rect_offset("m_Border"),
    ];
    if v4 {
        out.push(rect_offset("m_Margin"));
        out.push(rect_offset("m_Padding"));
    } else {
        out.push(rect_offset("m_Padding"));
        out.push(rect_offset("m_Margin"));
    }
    out.push(rect_offset("m_Overflow"));
    out.push(pptr_field("m_Font", "Font", version));
    if v4 {
        out.push(int_f("m_FontSize"));
        out.push(int_f("m_FontStyle"));
        out.push(int_f("m_Alignment"));
        out.push(bool_f("m_WordWrap", false));
        out.push(bool_f("m_RichText", true));
    } else {
        out.push(int_f("m_ImagePosition"));
        out.push(int_f("m_Alignment"));
        out.push(bool_f("m_WordWrap", true));
    }
    out.push(int_f("m_TextClipping"));
    if v4 {
        out.push(int_f("m_ImagePosition"));
    }
    out.push(vector2f("m_ContentOffset"));
    if !v4 {
        out.push(vector2f("m_ClipOffset"));
    }
    out.push(float_f("m_FixedWidth"));
    out.push(float_f("m_FixedHeight"));
    if v4 {
        out.push(bool_f("m_StretchWidth", false));
    } else {
        out.push(int_f("m_FontSize"));
        out.push(int_f("m_FontStyle"));
        out.push(bool_f("m_StretchWidth", true));
    }
    out.push(bool_f("m_StretchHeight", true));
    out
}

fn keyframe(name: &str, version: &UnityVersion) -> TemplateField {
    let mut fields = vec![
        float_f("time"),
        float_f("value"),
        float_f("inSlope"),
        float_f("outSlope"),
    ];
    if version.major >= 2018 {
        fields.push(int_f("weightedMode"));
        fields.push(float_f("inWeight"));
        fields.push(float_f("outWeight"));
    }
    node(name, "Keyframe", false, fields)
}

fn gradient_children(version: &UnityVersion) -> Vec<TemplateField> {
    if version.major > 5 || (version.major == 5 && version.minor >= 6) {
        let mut fields = Vec::new();
        for i in 0..8 {
            fields.push(rgbaf(&format!("key{i}")));
        }
        for i in 0..8 {
            fields.push(ushort(&format!("ctime{i}")));
        }
        for i in 0..8 {
            fields.push(ushort(&format!("atime{i}")));
        }
        if version.major > 2022 || (version.major == 2022 && version.minor >= 2) {
            fields.push(byte_f("m_Mode", false));
            fields.push(sbyte("m_ColorSpace", false));
        } else {
            fields.push(int_f("m_Mode"));
        }
        fields.push(byte_f("m_NumColorKeys", false));
        fields.push(byte_f("m_NumAlphaKeys", true));
        fields
    } else {
        (0..8)
            .map(|i| node(&format!("key{i}"), "ColorRGBA", false, vec![uint_f("rgba")]))
            .collect()
    }
}

fn animation_curve_children(version: &UnityVersion) -> Vec<TemplateField> {
    let mut fields = vec![
        vector(keyframe("m_Curve", version)),
        int_f("m_PreInfinity"),
        int_f("m_PostInfinity"),
    ];
    if version.major > 5 || (version.major == 5 && version.minor >= 3) {
        fields.push(int_f("m_RotationOrder"));
    }
    fields
}

/// `SpecialUnity`: children for a built-in Unity value type identified by its
/// short type name. Types not listed here (Vector2/3/4, Quaternion, Color,
/// Matrix4x4) fall through to reading their own serialized fields.
pub(crate) fn special_unity_children(
    type_name: &str,
    version: &UnityVersion,
) -> Option<Vec<TemplateField>> {
    Some(match type_name {
        "Gradient" => gradient_children(version),
        "AnimationCurve" => animation_curve_children(version),
        "LayerMask" => vec![uint_f("m_Bits")],
        "Bounds" => vec![vector3f("m_Center"), vector3f("m_Extent")],
        "BoundsInt" => vec![vector3int("m_Position"), vector3int("m_Size")],
        "Rect" => vec![
            float_f("x"),
            float_f("y"),
            float_f("width"),
            float_f("height"),
        ],
        "RectOffset" => vec![
            int_f("m_Left"),
            int_f("m_Right"),
            int_f("m_Top"),
            int_f("m_Bottom"),
        ],
        "Color32" => vec![uint_f("rgba")],
        "Vector2Int" => vec![int_f("x"), int_f("y")],
        "Vector3Int" => vec![int_f("x"), int_f("y"), int_f("z")],
        "PropertyName" => vec![string_field("id")],
        "GUIStyle" => gui_style_children(version),
        _ => return None,
    })
}
