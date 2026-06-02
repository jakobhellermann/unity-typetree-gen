// TODO(ai-review): review for style and correctness
//! Port of AssetsTools.NET `MonoCecilTempGenerator`, reading .NET metadata via
//! dotnetdll instead of Mono.Cecil.
//!
//! Field types are resolved across assemblies: a reference to e.g.
//! `UnityEngine.Object` or a special Unity type is looked up in the assembly
//! that defines it (parsed on demand by the [`AssemblyTypeTreeGenerator`]),
//! exactly as Cecil's assembly resolver does. Closed generics are handled only
//! for `List<T>`; open generic type parameters used as field types are not
//! solidified.
use crate::assembly::AssemblyTypeTreeGenerator;
use crate::template::*;
use crate::version::UnityVersion;
use dotnetdll::prelude::Resolution;
use dotnetdll::resolved::Accessibility as Access;
use dotnetdll::resolved::attribute::Attribute;
use dotnetdll::resolved::members::{Accessibility, Field, MethodReferenceParent, UserMethod};
use dotnetdll::resolved::types::{
    BaseType, Kind, MemberType, MethodType, ResolutionScope, TypeDefinition, TypeImplementation,
    TypeSource, UserType,
};

/// Base types at which inherited-field walking stops.
const BASE_STOP: &[&str] = &[
    "System.Object",
    "UnityEngine.Object",
    "UnityEngine.MonoBehaviour",
    "UnityEngine.ScriptableObject",
];

/// A resolved type: its definition together with the resolution (assembly) it
/// lives in, so further references can be resolved relative to it.
type Resolved = (
    &'static Resolution<'static>,
    &'static TypeDefinition<'static>,
);

/// The context for reading a type's fields: the resolution it lives in, plus
/// the generic arguments bound to its type parameters (and where those
/// arguments themselves resolve, i.e. the instantiation site's assembly).
#[derive(Clone, Copy)]
struct TypeCtx {
    res: &'static Resolution<'static>,
    args: &'static [MemberType],
    args_res: &'static Resolution<'static>,
}

impl TypeCtx {
    fn root(res: &'static Resolution<'static>) -> Self {
        TypeCtx {
            res,
            args: &[],
            args_res: res,
        }
    }
}

/// A resolved type together with the generic arguments to read it with.
struct ResolvedType {
    res: &'static Resolution<'static>,
    def: &'static TypeDefinition<'static>,
    args: &'static [MemberType],
    args_res: &'static Resolution<'static>,
}

impl ResolvedType {
    fn ctx(&self) -> TypeCtx {
        TypeCtx {
            res: self.res,
            args: self.args,
            args_res: self.args_res,
        }
    }
}

/// Replace an open generic parameter `T`(n) with the concrete argument bound in
/// `ctx`; other member types pass through unchanged.
fn effective(member: &'static MemberType, ctx: TypeCtx) -> (&'static MemberType, TypeCtx) {
    if let MemberType::TypeGeneric(n) = member
        && let Some(arg) = ctx.args.get(*n)
    {
        return (
            arg,
            TypeCtx {
                res: ctx.args_res,
                args: &[],
                args_res: ctx.args_res,
            },
        );
    }
    (member, ctx)
}

pub(crate) struct Generator<'r> {
    assemblies: &'r AssemblyTypeTreeGenerator,
    version: UnityVersion,
    using_managed_reference: bool,
}

impl<'r> Generator<'r> {
    pub(crate) fn new(assemblies: &'r AssemblyTypeTreeGenerator, version: UnityVersion) -> Self {
        Generator {
            assemblies,
            version,
            using_managed_reference: false,
        }
    }

    /// The serialized fields of `namespace.type_name` in `primary` (the
    /// MonoBehaviour header is added by the caller).
    pub(crate) fn read(
        &mut self,
        primary: &'static Resolution<'static>,
        namespace: &str,
        type_name: &str,
    ) -> Vec<TemplateField> {
        let mut children = Vec::new();
        if let Some(def) = find_type(primary, namespace, type_name) {
            let limit = self.version.serialization_limit();
            self.recursive_type_load(TypeCtx::root(primary), def, &mut children, limit, true);
        }
        if self.using_managed_reference {
            children.push(managed_references_registry("references", &self.version));
        }
        children
    }

    fn recursive_type_load(
        &mut self,
        ctx: TypeCtx,
        def: &'static TypeDefinition<'static>,
        out: &mut Vec<TemplateField>,
        available_depth: i32,
        is_recursive_call: bool,
    ) {
        let depth = if is_recursive_call {
            available_depth
        } else {
            available_depth - 1
        };
        if let Some(base) = self.inherited_base(ctx, def) {
            self.recursive_type_load(base.ctx(), base.def, out, depth, true);
        }
        out.extend(self.read_types(ctx, def, depth));
    }

    /// The base type to read inherited fields from, or `None` at a stop type or
    /// an unresolvable base.
    fn inherited_base(
        &self,
        ctx: TypeCtx,
        def: &'static TypeDefinition<'static>,
    ) -> Option<ResolvedType> {
        let ts = def.extends.as_ref()?;
        if BASE_STOP.contains(&source_type_name(ts, ctx.res).as_str()) {
            return None;
        }
        self.resolve_source_in(ts, ctx)
    }

    fn read_types(
        &mut self,
        ctx: TypeCtx,
        def: &'static TypeDefinition<'static>,
        available_depth: i32,
    ) -> Vec<TemplateField> {
        let mut out = Vec::new();
        for fi in self.acceptable_fields(ctx, def, available_depth) {
            let field = &def.fields[fi];

            let mut element = &field.return_type;
            let mut is_array_or_list = false;
            if let Some(elem) = vector_element(&field.return_type) {
                is_array_or_list = true;
                element = elem;
            } else if let Some(elem) = list_element(&field.return_type, ctx.res) {
                is_array_or_list = true;
                element = elem;
            }

            let class = self.classify(field, element, ctx, available_depth);
            let mut node = TemplateField {
                name: field.name.to_string(),
                aligned: type_aligns_by_name(&class.ty),
                ty: class.ty,
                children: class.children,
            };

            if is_array_or_list {
                node = if class.primitive || class.string || class.derives_ueobject {
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
        field: &Field,
        element: &'static MemberType,
        ctx: TypeCtx,
        available_depth: i32,
    ) -> Classified {
        // Substitute an open generic parameter (`T`) with the concrete argument.
        let (element, ectx) = effective(element, ctx);

        if let Some(name) = base_primitive_name(element) {
            return Classified::leaf(name.to_string(), true, false, false);
        }
        if is_string(element) {
            return Classified {
                ty: "string".to_string(),
                primitive: false,
                string: true,
                derives_ueobject: false,
                children: string_children(),
            };
        }
        let Some(rt) = self.resolve_concrete(element, ectx) else {
            return Classified::leaf(String::new(), false, false, false);
        };

        if let Some(under) = self.enum_underlying(rt.res, rt.def) {
            let ty = convert_base_to_primitive(&under)
                .unwrap_or("int")
                .to_string();
            return Classified::leaf(ty, true, false, false);
        }
        if self.derives_from_ueobject(rt.res, rt.def) {
            let ty = format!("PPtr<${}>", rt.def.name);
            return Classified {
                ty,
                primitive: false,
                string: false,
                derives_ueobject: true,
                children: pptr_children(&self.version),
            };
        }
        if field_has_serialize_reference(field, ctx.res) {
            self.using_managed_reference = true;
            return Classified {
                ty: "managedReference".to_string(),
                primitive: false,
                string: false,
                derives_ueobject: false,
                children: managed_reference_children(&self.version),
            };
        }

        let def = rt.def;
        let full_name = def.type_name();
        let children = if is_special_unity_type(&full_name) {
            special_unity_children(&def.name, &self.version)
                .unwrap_or_else(|| self.serialized(rt, available_depth))
        } else if def.flags.serializable {
            self.serialized(rt, available_depth)
        } else {
            Vec::new()
        };
        Classified {
            ty: def.name.to_string(),
            primitive: false,
            string: false,
            derives_ueobject: false,
            children,
        }
    }

    fn serialized(&mut self, rt: ResolvedType, available_depth: i32) -> Vec<TemplateField> {
        let mut out = Vec::new();
        self.recursive_type_load(rt.ctx(), rt.def, &mut out, available_depth, false);
        out
    }

    fn acceptable_fields(
        &self,
        ctx: TypeCtx,
        def: &'static TypeDefinition<'static>,
        available_depth: i32,
    ) -> Vec<usize> {
        let mut valid = Vec::new();
        for (fi, field) in def.fields.iter().enumerate() {
            let is_public = matches!(field.accessibility, Accessibility::Access(Access::Public));
            let has_serialize_attr = field_has_attr(field, ctx.res, "UnityEngine.SerializeField")
                || field_has_attr(field, ctx.res, "UnityEngine.SerializeReference");
            if !(is_public || has_serialize_attr) {
                continue;
            }
            if field.static_member || field.not_serialized || field.init_only || field.literal {
                continue;
            }

            let check = if let Some(elem) = collection_element(&field.return_type, ctx.res) {
                if available_depth < 0 {
                    continue;
                }
                if collection_element(elem, ctx.res).is_some() {
                    continue; // Unity can't serialize collections of collections
                }
                elem
            } else {
                if self.member_is_same_type(&field.return_type, ctx, def)
                    && !self.derives_from_ueobject(ctx.res, def)
                {
                    continue; // self-typed field on a non-UnityEngine.Object type
                }
                &field.return_type
            };

            if self.is_valid_def(field, check, ctx, available_depth) {
                valid.push(fi);
            }
        }
        valid
    }

    fn is_valid_def(
        &self,
        field: &Field,
        member: &'static MemberType,
        ctx: TypeCtx,
        available_depth: i32,
    ) -> bool {
        let (member, ectx) = effective(member, ctx);
        if base_primitive_name(member).is_some() || is_string(member) {
            return true;
        }
        let Some(rt) = self.resolve_concrete(member, ectx) else {
            return false;
        };

        if !rt.def.generic_parameters.is_empty() && self.version.major < 2020 {
            return false;
        }
        if let Some(under) = self.enum_underlying(rt.res, rt.def) {
            return under != "System.Int64" && under != "System.UInt64";
        }

        let full_name = rt.def.type_name();
        if available_depth < 0 {
            return is_value_type(rt.res, rt.def)
                && (rt.def.flags.serializable || is_special_unity_type(&full_name));
        }
        if self.derives_from_ueobject(rt.res, rt.def) || is_special_unity_type(&full_name) {
            return true;
        }
        if field_has_serialize_reference(field, ctx.res) {
            return !is_value_type(rt.res, rt.def) && rt.def.generic_parameters.is_empty();
        }
        if is_assembly_blacklisted(assembly_name(rt.res)) {
            return false;
        }
        !rt.def.flags.abstract_type && rt.def.flags.serializable
    }

    fn member_is_same_type(
        &self,
        member: &'static MemberType,
        ctx: TypeCtx,
        def: &'static TypeDefinition<'static>,
    ) -> bool {
        matches!(self.resolve_member(member, ctx), Some(rt) if std::ptr::eq(rt.def, def))
    }

    // --- type predicates (cross-assembly aware) ---

    fn derives_from_ueobject(
        &self,
        res: &'static Resolution<'static>,
        def: &'static TypeDefinition<'static>,
    ) -> bool {
        if matches!(def.flags.kind, Kind::Interface) {
            return false;
        }
        if def.type_name() == "UnityEngine.Object" {
            return true;
        }
        let Some(ts) = def.extends.as_ref() else {
            return false;
        };
        let base = source_type_name(ts, res);
        if base == "UnityEngine.Object" {
            return true;
        }
        if base == "System.Object" {
            return false;
        }
        match self.resolve_source_in(ts, TypeCtx::root(res)) {
            Some(base) => self.derives_from_ueobject(base.res, base.def),
            None => false,
        }
    }

    fn enum_underlying(
        &self,
        res: &'static Resolution<'static>,
        def: &'static TypeDefinition<'static>,
    ) -> Option<String> {
        if base_full_name(res, def).as_deref() != Some("System.Enum") {
            return None;
        }
        let value_field = def.fields.iter().find(|f| f.name == "value__")?;
        base_primitive_full_name(&value_field.return_type)
    }

    // --- resolution ---

    /// Resolve a field's member type, substituting an open generic parameter
    /// with its concrete argument from `ctx` first.
    fn resolve_member(&self, member: &'static MemberType, ctx: TypeCtx) -> Option<ResolvedType> {
        let (member, ctx) = effective(member, ctx);
        self.resolve_concrete(member, ctx)
    }

    /// Resolve an already-concrete member type (no generic substitution) to its
    /// definition, carrying any generic arguments for further field resolution.
    fn resolve_concrete(&self, member: &'static MemberType, ctx: TypeCtx) -> Option<ResolvedType> {
        let MemberType::Base(b) = member else {
            return None;
        };
        match &**b {
            BaseType::Type { source, .. } => self.resolve_source_in(source, ctx),
            _ => None,
        }
    }

    fn resolve_source_in(
        &self,
        source: &'static TypeSource<MemberType>,
        ctx: TypeCtx,
    ) -> Option<ResolvedType> {
        let (user, args) = match source {
            TypeSource::User(user) => (user, [].as_slice()),
            TypeSource::Generic { base, parameters } => (base, parameters.as_slice()),
        };
        let (res, def) = self.resolve_user(user, ctx.res)?;
        // Generic arguments are written at the instantiation site, so they
        // resolve in `ctx.res` even when `def` lives in another assembly.
        Some(ResolvedType {
            res,
            def,
            args,
            args_res: ctx.res,
        })
    }

    fn resolve_user(&self, user: &UserType, res: &'static Resolution<'static>) -> Option<Resolved> {
        match user {
            UserType::Definition(idx) => Some((res, &res[*idx])),
            UserType::Reference(idx) => {
                let type_ref = &res[*idx];
                let namespace = type_ref.namespace.as_deref().unwrap_or("");
                let target = match &type_ref.scope {
                    ResolutionScope::Assembly(assembly) => {
                        let name = format!("{}.dll", res[*assembly].name);
                        self.assemblies.resolution(&name)?
                    }
                    ResolutionScope::CurrentModule => res,
                    _ => return None,
                };
                self.find_type_following_forwards(target, namespace, &type_ref.name)
            }
        }
    }

    /// Find `namespace.name` in `res`, following type-forwarder exports across
    /// assemblies (e.g. `UnityEngine.dll` forwards `ScriptableObject` to
    /// `UnityEngine.CoreModule`).
    fn find_type_following_forwards(
        &self,
        res: &'static Resolution<'static>,
        namespace: &str,
        name: &str,
    ) -> Option<Resolved> {
        if let Some(def) = find_type(res, namespace, name) {
            return Some((res, def));
        }
        let exported = res
            .exported_types
            .iter()
            .find(|e| e.name == name && e.namespace.as_deref().unwrap_or("") == namespace)?;
        if let TypeImplementation::TypeForwarder(assembly) = &exported.implementation {
            let target = self
                .assemblies
                .resolution(&format!("{}.dll", res[*assembly].name))?;
            return self.find_type_following_forwards(target, namespace, name);
        }
        None
    }
}

struct Classified {
    ty: String,
    primitive: bool,
    string: bool,
    derives_ueobject: bool,
    children: Vec<TemplateField>,
}

impl Classified {
    fn leaf(ty: String, primitive: bool, string: bool, derives_ueobject: bool) -> Classified {
        Classified {
            ty,
            primitive,
            string,
            derives_ueobject,
            children: Vec::new(),
        }
    }
}

fn find_type(
    res: &'static Resolution<'static>,
    namespace: &str,
    type_name: &str,
) -> Option<&'static TypeDefinition<'static>> {
    res.type_definitions
        .iter()
        .find(|td| td.name == type_name && td.namespace.as_deref().unwrap_or("") == namespace)
}

fn source_type_name(ts: &TypeSource<MemberType>, res: &Resolution) -> String {
    match ts {
        TypeSource::User(user) => user.type_name(res),
        TypeSource::Generic { base, .. } => base.type_name(res),
    }
}

fn base_full_name(res: &Resolution, def: &TypeDefinition) -> Option<String> {
    def.extends.as_ref().map(|ts| source_type_name(ts, res))
}

fn is_value_type(res: &Resolution, def: &TypeDefinition) -> bool {
    matches!(
        base_full_name(res, def).as_deref(),
        Some("System.ValueType") | Some("System.Enum")
    )
}

fn assembly_name<'a>(res: &'a Resolution<'a>) -> &'a str {
    res.assembly.as_ref().map(|a| a.name.as_ref()).unwrap_or("")
}

// --- attributes ---

fn field_has_serialize_reference(field: &Field, res: &Resolution) -> bool {
    field_has_attr(field, res, "UnityEngine.SerializeReference")
}

fn field_has_attr(field: &Field, res: &Resolution, full_name: &str) -> bool {
    field
        .attributes
        .iter()
        .any(|a| attribute_type_name(a, res).as_deref() == Some(full_name))
}

fn attribute_type_name(attr: &Attribute, res: &Resolution) -> Option<String> {
    match attr.constructor {
        UserMethod::Definition(m) => Some(res[m.parent_type()].type_name()),
        UserMethod::Reference(r) => match &res[r].parent {
            MethodReferenceParent::Type(mt) => method_type_name(mt, res),
            _ => None,
        },
    }
}

// --- free helpers over signatures ---

fn collection_element<'m>(member: &'m MemberType, res: &Resolution) -> Option<&'m MemberType> {
    vector_element(member).or_else(|| list_element(member, res))
}

fn vector_element(member: &MemberType) -> Option<&MemberType> {
    if let MemberType::Base(b) = member
        && let BaseType::Vector(_, elem) = &**b
    {
        return Some(elem);
    }
    None
}

fn list_element<'m>(member: &'m MemberType, res: &Resolution) -> Option<&'m MemberType> {
    if let MemberType::Base(b) = member
        && let BaseType::Type {
            source: TypeSource::Generic { base, parameters },
            ..
        } = &**b
        && base.type_name(res) == "System.Collections.Generic.List`1"
    {
        return parameters.first();
    }
    None
}

fn is_string(member: &MemberType) -> bool {
    matches!(member, MemberType::Base(b) if matches!(&**b, BaseType::String))
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
            TypeSource::User(user) => user.type_name(res),
            TypeSource::Generic { base, .. } => base.type_name(res),
        });
    }
    None
}
