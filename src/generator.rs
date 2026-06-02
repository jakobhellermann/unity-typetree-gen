// TODO(ai-review): review for style and correctness
//! Port of AssetsTools.NET `MonoCecilTempGenerator`, reading .NET metadata via
//! dotnetdll instead of Mono.Cecil.
//!
//! Scope: closed generics are handled only for `List<T>` (the one case Unity
//! serializes); open generic type parameters used as field types are not
//! solidified. That covers everything Unity actually serializes here.
use crate::template::*;
use crate::version::UnityVersion;
use dotnetdll::prelude::{Resolution, TypeIndex};
use dotnetdll::resolved::Accessibility as Access;
use dotnetdll::resolved::attribute::Attribute;
use dotnetdll::resolved::members::{Accessibility, MethodReferenceParent, UserMethod};
use dotnetdll::resolved::types::{BaseType, Kind, MemberType, MethodType, TypeSource, UserType};

/// Base types at which inherited-field walking stops.
const BASE_STOP: &[&str] = &[
    "System.Object",
    "UnityEngine.Object",
    "UnityEngine.MonoBehaviour",
    "UnityEngine.ScriptableObject",
];

pub(crate) struct Generator<'a, 'r> {
    res: &'r Resolution<'a>,
    version: UnityVersion,
    using_managed_reference: bool,
}

impl<'a, 'r> Generator<'a, 'r> {
    pub(crate) fn read(
        res: &'r Resolution<'a>,
        namespace: &str,
        type_name: &str,
        version: UnityVersion,
    ) -> Vec<TemplateField> {
        let mut g = Generator {
            res,
            version,
            using_managed_reference: false,
        };
        let mut children = Vec::new();
        if let Some(idx) = g.find_type(namespace, type_name) {
            let limit = version.serialization_limit();
            g.recursive_type_load(idx, &mut children, limit, true);
        }
        if g.using_managed_reference {
            children.push(managed_references_registry("references", &version));
        }
        children
    }

    fn find_type(&self, namespace: &str, type_name: &str) -> Option<TypeIndex> {
        self.res.enumerate_type_definitions().find_map(|(idx, td)| {
            let ns = td.namespace.as_deref().unwrap_or("");
            (td.name == type_name && ns == namespace).then_some(idx)
        })
    }

    fn recursive_type_load(
        &mut self,
        idx: TypeIndex,
        out: &mut Vec<TemplateField>,
        available_depth: i32,
        is_recursive_call: bool,
    ) {
        let depth = if is_recursive_call {
            available_depth
        } else {
            available_depth - 1
        };
        if let Some(base_idx) = self.inherited_base(idx) {
            self.recursive_type_load(base_idx, out, depth, true);
        }
        out.extend(self.read_types(idx, depth));
    }

    /// The base type to read inherited fields from, or `None` at a stop type or
    /// an unresolvable (external) base.
    fn inherited_base(&self, idx: TypeIndex) -> Option<TypeIndex> {
        let ts = self.res[idx].extends.as_ref()?;
        if BASE_STOP.contains(&self.source_type_name(ts).as_str()) {
            return None;
        }
        user_definition(ts)
    }

    fn read_types(&mut self, idx: TypeIndex, available_depth: i32) -> Vec<TemplateField> {
        let res = self.res;
        let mut out = Vec::new();
        for fi in self.acceptable_fields(idx, available_depth) {
            let field = &res[idx].fields[fi];

            let mut element = &field.return_type;
            let mut is_array_or_list = false;
            if let Some(elem) = vector_element(&field.return_type) {
                is_array_or_list = true;
                element = elem;
            } else if let Some(elem) = self.list_element(&field.return_type) {
                is_array_or_list = true;
                element = elem;
            }

            let class = self.classify(field, element, available_depth);
            let mut node = TemplateField {
                name: field.name.to_string(),
                aligned: type_aligns_by_name(&class.ty),
                ty: class.ty,
                children: class.children,
            };

            if is_array_or_list {
                // AssetsTools.NET 1.0.1 wraps only primitive and PPtr element
                // arrays as a bare `vector`; everything else (string included)
                // keeps the element type via VectorWithType.
                node = if class.primitive || class.derives_ueobject {
                    vector(node)
                } else {
                    vector_with_type(node)
                };
            }
            out.push(node);
        }
        out
    }

    fn classify(
        &mut self,
        field: &dotnetdll::resolved::members::Field,
        element: &MemberType,
        available_depth: i32,
    ) -> Classified {
        if let Some(name) = base_primitive_name(element) {
            return Classified::leaf(name.to_string(), true, false);
        }
        if is_string(element) {
            return Classified {
                ty: "string".to_string(),
                primitive: false,
                derives_ueobject: false,
                children: string_children(),
            };
        }
        let Some(tidx) = user_definition_of(element) else {
            return Classified::leaf(String::new(), false, false);
        };

        if let Some(under) = self.enum_underlying(tidx) {
            let ty = convert_base_to_primitive(&under)
                .unwrap_or("int")
                .to_string();
            return Classified::leaf(ty, true, false);
        }
        if self.derives_from_ueobject(tidx) {
            let ty = format!("PPtr<${}>", self.res[tidx].name);
            return Classified {
                ty,
                primitive: false,
                derives_ueobject: true,
                children: pptr_children(&self.version),
            };
        }
        if self.field_has_serialize_reference(field) {
            self.using_managed_reference = true;
            return Classified {
                ty: "managedReference".to_string(),
                primitive: false,
                derives_ueobject: false,
                children: managed_reference_children(&self.version),
            };
        }

        let full_name = self.res[tidx].type_name();
        let ty = self.res[tidx].name.to_string();
        let children = if is_special_unity_type(&full_name) {
            special_unity_children(self.res[tidx].name.as_ref(), &self.version)
                .unwrap_or_else(|| self.serialized(tidx, available_depth))
        } else if self.res[tidx].flags.serializable {
            self.serialized(tidx, available_depth)
        } else {
            Vec::new()
        };
        Classified {
            ty,
            primitive: false,
            derives_ueobject: false,
            children,
        }
    }

    fn serialized(&mut self, idx: TypeIndex, available_depth: i32) -> Vec<TemplateField> {
        let mut out = Vec::new();
        self.recursive_type_load(idx, &mut out, available_depth, false);
        out
    }

    fn acceptable_fields(&self, idx: TypeIndex, available_depth: i32) -> Vec<usize> {
        let res = self.res;
        let mut valid = Vec::new();
        for (fi, field) in res[idx].fields.iter().enumerate() {
            let is_public = matches!(field.accessibility, Accessibility::Access(Access::Public));
            let has_serialize_attr = self.field_has_attr(field, "UnityEngine.SerializeField")
                || self.field_has_attr(field, "UnityEngine.SerializeReference");
            if !(is_public || has_serialize_attr) {
                continue;
            }
            if field.static_member || field.not_serialized || field.init_only || field.literal {
                continue;
            }

            let check = if let Some(elem) = self.collection_element(&field.return_type) {
                if available_depth < 0 {
                    continue;
                }
                if self.collection_element(elem).is_some() {
                    continue; // Unity can't serialize collections of collections
                }
                elem
            } else {
                if user_definition_of(&field.return_type) == Some(idx)
                    && !self.derives_from_ueobject(idx)
                {
                    continue; // self-typed field on a non-UnityEngine.Object type
                }
                &field.return_type
            };

            if self.is_valid_def(field, check, available_depth) {
                valid.push(fi);
            }
        }
        valid
    }

    fn is_valid_def(
        &self,
        field: &dotnetdll::resolved::members::Field,
        member: &MemberType,
        available_depth: i32,
    ) -> bool {
        if base_primitive_name(member).is_some() || is_string(member) {
            return true;
        }
        let Some(tidx) = user_definition_of(member) else {
            return false;
        };
        let def = &self.res[tidx];

        if !def.generic_parameters.is_empty() && self.version.major < 2020 {
            return false;
        }
        if let Some(under) = self.enum_underlying(tidx) {
            return under != "System.Int64" && under != "System.UInt64";
        }

        let full_name = def.type_name();
        if available_depth < 0 {
            return self.is_value_type(tidx)
                && (def.flags.serializable || is_special_unity_type(&full_name));
        }
        if self.derives_from_ueobject(tidx) || is_special_unity_type(&full_name) {
            return true;
        }
        if self.field_has_serialize_reference(field) {
            return !self.is_value_type(tidx) && def.generic_parameters.is_empty();
        }
        if is_assembly_blacklisted(self.assembly_name()) {
            return false;
        }
        !def.flags.abstract_type && def.flags.serializable
    }

    // --- type predicates ---

    fn derives_from_ueobject(&self, idx: TypeIndex) -> bool {
        let def = &self.res[idx];
        if matches!(def.flags.kind, Kind::Interface) {
            return false;
        }
        if def.type_name() == "UnityEngine.Object" {
            return true;
        }
        let Some(ts) = def.extends.as_ref() else {
            return false;
        };
        let base = self.source_type_name(ts);
        if base == "UnityEngine.Object" {
            return true;
        }
        if base == "System.Object" {
            return false;
        }
        match user_definition(ts) {
            Some(base_idx) => self.derives_from_ueobject(base_idx),
            None => false,
        }
    }

    fn is_value_type(&self, idx: TypeIndex) -> bool {
        matches!(
            self.base_full_name(idx).as_deref(),
            Some("System.ValueType") | Some("System.Enum")
        )
    }

    /// `Some(underlying .NET full name)` if `idx` is an enum, else `None`.
    fn enum_underlying(&self, idx: TypeIndex) -> Option<String> {
        if self.base_full_name(idx).as_deref() != Some("System.Enum") {
            return None;
        }
        let value_field = self.res[idx].fields.iter().find(|f| f.name == "value__")?;
        base_primitive_full_name(&value_field.return_type)
    }

    fn base_full_name(&self, idx: TypeIndex) -> Option<String> {
        self.res[idx]
            .extends
            .as_ref()
            .map(|ts| self.source_type_name(ts))
    }

    fn source_type_name(&self, ts: &TypeSource<MemberType>) -> String {
        match ts {
            TypeSource::User(ut) => ut.type_name(self.res),
            TypeSource::Generic { base, .. } => base.type_name(self.res),
        }
    }

    fn assembly_name(&self) -> &str {
        self.res
            .assembly
            .as_ref()
            .map(|a| a.name.as_ref())
            .unwrap_or("")
    }

    // --- attributes ---

    fn field_has_serialize_reference(&self, field: &dotnetdll::resolved::members::Field) -> bool {
        self.field_has_attr(field, "UnityEngine.SerializeReference")
    }

    fn field_has_attr(&self, field: &dotnetdll::resolved::members::Field, full_name: &str) -> bool {
        field
            .attributes
            .iter()
            .any(|a| self.attribute_type_name(a).as_deref() == Some(full_name))
    }

    fn attribute_type_name(&self, attr: &Attribute) -> Option<String> {
        match attr.constructor {
            UserMethod::Definition(m) => Some(self.res[m.parent_type()].type_name()),
            UserMethod::Reference(r) => match &self.res[r].parent {
                MethodReferenceParent::Type(mt) => method_type_name(mt, self.res),
                _ => None,
            },
        }
    }

    fn collection_element<'m>(&self, member: &'m MemberType) -> Option<&'m MemberType> {
        vector_element(member).or_else(|| self.list_element(member))
    }

    fn list_element<'m>(&self, member: &'m MemberType) -> Option<&'m MemberType> {
        if let MemberType::Base(b) = member
            && let BaseType::Type {
                source: TypeSource::Generic { base, parameters },
                ..
            } = &**b
            && base.type_name(self.res) == "System.Collections.Generic.List`1"
        {
            return parameters.first();
        }
        None
    }
}

struct Classified {
    ty: String,
    primitive: bool,
    derives_ueobject: bool,
    children: Vec<TemplateField>,
}

impl Classified {
    fn leaf(ty: String, primitive: bool, derives_ueobject: bool) -> Classified {
        Classified {
            ty,
            primitive,
            derives_ueobject,
            children: Vec::new(),
        }
    }
}

// --- free helpers over signatures (no resolution needed) ---

fn vector_element(member: &MemberType) -> Option<&MemberType> {
    if let MemberType::Base(b) = member
        && let BaseType::Vector(_, elem) = &**b
    {
        return Some(elem);
    }
    None
}

fn is_string(member: &MemberType) -> bool {
    matches!(member, MemberType::Base(b) if matches!(&**b, BaseType::String))
}

fn user_definition_of(member: &MemberType) -> Option<TypeIndex> {
    if let MemberType::Base(b) = member
        && let BaseType::Type {
            source: TypeSource::User(UserType::Definition(idx)),
            ..
        } = &**b
    {
        return Some(*idx);
    }
    None
}

fn user_definition(ts: &TypeSource<MemberType>) -> Option<TypeIndex> {
    match ts {
        TypeSource::User(UserType::Definition(idx)) => Some(*idx),
        _ => None,
    }
}

fn base_primitive_name(member: &MemberType) -> Option<&'static str> {
    if let MemberType::Base(b) = member {
        return base_type_primitive_name(b);
    }
    None
}

fn base_type_primitive_name(b: &BaseType<MemberType>) -> Option<&'static str> {
    Some(match b {
        BaseType::Boolean => "UInt8",
        BaseType::Char => "UInt16",
        BaseType::Int8 => "SInt8",
        BaseType::UInt8 => "UInt8",
        BaseType::Int16 => "SInt16",
        BaseType::UInt16 => "UInt16",
        BaseType::Int32 => "int",
        BaseType::UInt32 => "unsigned int",
        BaseType::Int64 => "SInt64",
        BaseType::UInt64 => "UInt64",
        BaseType::Float32 => "float",
        BaseType::Float64 => "double",
        _ => return None,
    })
}

fn base_primitive_full_name(member: &MemberType) -> Option<String> {
    let MemberType::Base(b) = member else {
        return None;
    };
    Some(
        match &**b {
            BaseType::Boolean => "System.Boolean",
            BaseType::Char => "System.Char",
            BaseType::Int8 => "System.SByte",
            BaseType::UInt8 => "System.Byte",
            BaseType::Int16 => "System.Int16",
            BaseType::UInt16 => "System.UInt16",
            BaseType::Int32 => "System.Int32",
            BaseType::UInt32 => "System.UInt32",
            BaseType::Int64 => "System.Int64",
            BaseType::UInt64 => "System.UInt64",
            BaseType::Float32 => "System.Single",
            BaseType::Float64 => "System.Double",
            _ => return None,
        }
        .to_string(),
    )
}

fn method_type_name(mt: &MethodType, res: &Resolution) -> Option<String> {
    if let MethodType::Base(b) = mt
        && let BaseType::Type { source, .. } = &**b
    {
        return Some(match source {
            TypeSource::User(ut) => ut.type_name(res),
            TypeSource::Generic { base, .. } => base.type_name(res),
        });
    }
    None
}
