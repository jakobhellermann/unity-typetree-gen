//! Port of AssetsTools.NET `MonoCecilTempGenerator`, reading .NET metadata via
//! dotnetdll instead of Mono.Cecil.
//!
//! Field types are resolved across assemblies: a reference to e.g.
//! `UnityEngine.Object` or a special Unity type is looked up in the assembly
//! that defines it (parsed on demand by the [`AssemblyTypeTreeGenerator`]),
//! exactly as Cecil's assembly resolver does.
//!
//! Open generic type parameters (`T`) are solidified to their concrete
//! arguments via [`TypeCtx`], which is a recursive substitution environment:
//! each bound argument carries its own context, so a parameter is substituted
//! before any structural inspection (collection unwrapping, validity) and a
//! generic base class binding its own parameters (`Derived<T> : Base<T>`)
//! propagates the argument into the base's fields.
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

/// A concrete generic argument: the member type bound to a type parameter,
/// together with the context it must itself be read in (its instantiation
/// site's assembly and any further argument bindings).
#[derive(Clone)]
struct GenericArg {
    member: &'static MemberType,
    ctx: TypeCtx,
}

/// The context for reading a type's fields: the resolution it lives in, plus
/// the generic arguments bound to its type parameters. Each argument carries
/// its own context, so substitution is a recursive environment — a base class
/// `Base<T>` reached from `Derived<float>` binds `T` to `float` read in the
/// `Derived` site, not to the open parameter.
#[derive(Clone)]
struct TypeCtx {
    res: &'static Resolution<'static>,
    args: std::rc::Rc<[GenericArg]>,
}

impl TypeCtx {
    fn root(res: &'static Resolution<'static>) -> Self {
        TypeCtx {
            res,
            args: std::rc::Rc::from([]),
        }
    }
}

/// A resolved type together with the context (arguments) to read it with.
struct ResolvedType {
    res: &'static Resolution<'static>,
    def: &'static TypeDefinition<'static>,
    ctx: TypeCtx,
}

impl ResolvedType {
    fn ctx(&self) -> TypeCtx {
        self.ctx.clone()
    }
}

/// Replace an open generic parameter `T`(n) with the concrete argument bound in
/// `ctx`, recursively — the argument is returned with its own context so a
/// substituted type that is itself open resolves further. Non-parameter member
/// types pass through unchanged.
fn effective(member: &'static MemberType, ctx: &TypeCtx) -> (&'static MemberType, TypeCtx) {
    if let MemberType::TypeGeneric(n) = member
        && let Some(arg) = ctx.args.get(*n)
    {
        return effective(arg.member, &arg.ctx);
    }
    (member, ctx.clone())
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
    /// MonoBehaviour header is added by the caller), or `None` if the type
    /// itself can't be resolved in the loaded assemblies (e.g. a MonoScript
    /// naming an editor-only or version-mismatched type) — distinct from a type
    /// that resolves but has no serialized fields (`Some(empty)`).
    pub(crate) fn read(
        &mut self,
        primary: &'static Resolution<'static>,
        namespace: &str,
        type_name: &str,
    ) -> Option<Vec<TemplateField>> {
        // A MonoScript may name a type by an assembly that only *forwards* it
        // (e.g. `UnityEngine.dll` forwards `FontAsset` to a module assembly), so
        // follow type-forwarder exports to find the real definition and read it
        // in the assembly it actually lives in.
        let (res, def) = self.find_type_following_forwards(primary, namespace, type_name)?;

        let mut children = Vec::new();
        let limit = self.version.serialization_limit();
        self.recursive_type_load(&TypeCtx::root(res), def, &mut children, limit, true);
        if self.using_managed_reference {
            children.push(managed_references_registry("references", &self.version));
        }
        Some(children)
    }

    fn recursive_type_load(
        &mut self,
        ctx: &TypeCtx,
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
            self.recursive_type_load(&base.ctx(), base.def, out, depth, true);
        }
        out.extend(self.read_types(ctx, def, depth));
    }

    /// The base type to read inherited fields from, or `None` at a stop type or
    /// an unresolvable base.
    fn inherited_base(
        &self,
        ctx: &TypeCtx,
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
        ctx: &TypeCtx,
        def: &'static TypeDefinition<'static>,
        available_depth: i32,
    ) -> Vec<TemplateField> {
        let mut out = Vec::new();
        for fi in self.acceptable_fields(ctx, def, available_depth) {
            let field = &def.fields[fi];

            // Substitute an open generic parameter (`T`) before any structural
            // inspection, so a collection-typed argument (`Holder<int[]>`) is
            // unwrapped on the concrete type, not the open one.
            let (member, ectx) = effective(&field.return_type, ctx);

            let mut element = member;
            let mut element_ctx = ectx.clone();
            let mut is_array_or_list = false;
            if let Some(elem) = vector_element(member) {
                is_array_or_list = true;
                element = elem;
            } else if let Some(elem) = list_element(member, ectx.res) {
                is_array_or_list = true;
                element = elem;
            }
            // A collection element may itself be an open parameter; substitute
            // again so the element classifies on its concrete type.
            if is_array_or_list {
                (element, element_ctx) = effective(element, &ectx);
            }

            let kind = self.classify(field, element, &element_ctx, ctx.res, available_depth);
            let ty = kind.type_name();
            let plain_vector = kind.collection_is_plain_vector();
            let mut node = TemplateField {
                name: field.name.to_string(),
                aligned: type_aligns_by_name(&ty),
                ty,
                children: kind.into_children(&self.version),
            };

            if is_array_or_list {
                node = if plain_vector {
                    vector(node)
                } else {
                    vector_with_type(node)
                };
            }
            out.push(node);
        }
        out
    }

    /// Classify the field's (already generic-substituted) element type into a
    /// template node. `element`/`element_ctx` are the substituted type and its
    /// context; `field_res` is the assembly the field is *declared* in, used to
    /// read the field's own attributes (e.g. `[SerializeReference]`).
    fn classify(
        &mut self,
        field: &Field,
        element: &'static MemberType,
        element_ctx: &TypeCtx,
        field_res: &'static Resolution<'static>,
        available_depth: i32,
    ) -> FieldKind {
        if let Some(name) = base_primitive_name(element) {
            return FieldKind::Primitive(name.to_string());
        }
        if is_string(element) {
            return FieldKind::String;
        }
        let Some(rt) = self.resolve_concrete(element, element_ctx) else {
            return FieldKind::Primitive(String::new());
        };

        if let Some(under) = self.enum_underlying(rt.res, rt.def) {
            let ty = convert_base_to_primitive(&under).unwrap_or("int");
            return FieldKind::Primitive(ty.to_string());
        }
        if self.derives_from_ueobject(rt.res, rt.def) {
            return FieldKind::PPtr(rt.def.name.to_string());
        }
        if field_has_serialize_reference(field, field_res) {
            self.using_managed_reference = true;
            return FieldKind::ManagedReference;
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
        FieldKind::Named {
            ty: def.name.to_string(),
            children,
        }
    }

    fn serialized(&mut self, rt: ResolvedType, available_depth: i32) -> Vec<TemplateField> {
        let mut out = Vec::new();
        self.recursive_type_load(&rt.ctx(), rt.def, &mut out, available_depth, false);
        out
    }

    fn acceptable_fields(
        &self,
        ctx: &TypeCtx,
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

            // Substitute generic parameters before structural checks, mirroring
            // the emit path in `read_types`: a collection-typed argument must be
            // detected on the concrete type, not the open parameter.
            let (member, ectx) = effective(&field.return_type, ctx);
            let check = if let Some(elem) = collection_element(member, ectx.res) {
                if available_depth < 0 {
                    continue;
                }
                let (elem, eectx) = effective(elem, &ectx);
                if collection_element(elem, eectx.res).is_some() {
                    continue; // Unity can't serialize collections of collections
                }
                elem
            } else {
                if self.member_is_same_type(member, &ectx, def)
                    && !self.derives_from_ueobject(ctx.res, def)
                {
                    continue; // self-typed field on a non-UnityEngine.Object type
                }
                member
            };

            if self.is_valid_def(field, check, &ectx, ctx.res, available_depth) {
                valid.push(fi);
            }
        }
        valid
    }

    /// Whether `member` (already generic-substituted, read in `member_ctx`)
    /// yields a serialized field. `field_res` is the field's declaring assembly,
    /// for reading its attributes.
    fn is_valid_def(
        &self,
        field: &Field,
        member: &'static MemberType,
        member_ctx: &TypeCtx,
        field_res: &'static Resolution<'static>,
        available_depth: i32,
    ) -> bool {
        if base_primitive_name(member).is_some() || is_string(member) {
            return true;
        }
        let Some(rt) = self.resolve_concrete(member, member_ctx) else {
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
        if field_has_serialize_reference(field, field_res) {
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
        ctx: &TypeCtx,
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
        match self.resolve_source_in(ts, &TypeCtx::root(res)) {
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
    fn resolve_member(&self, member: &'static MemberType, ctx: &TypeCtx) -> Option<ResolvedType> {
        let (member, ctx) = effective(member, ctx);
        self.resolve_concrete(member, &ctx)
    }

    /// Resolve an already-concrete member type (no generic substitution) to its
    /// definition, carrying any generic arguments for further field resolution.
    fn resolve_concrete(&self, member: &'static MemberType, ctx: &TypeCtx) -> Option<ResolvedType> {
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
        ctx: &TypeCtx,
    ) -> Option<ResolvedType> {
        let (user, parameters) = match source {
            TypeSource::User(user) => (user, [].as_slice()),
            TypeSource::Generic { base, parameters } => (base, parameters.as_slice()),
        };
        let (res, def) = self.resolve_user(user, ctx.res)?;
        // Each generic argument is bound in the current context: it is written
        // at this instantiation site (so it resolves in `ctx.res`), and an
        // argument that is itself an open parameter (`Base<T>` reached from
        // `Derived<float>`) is substituted through `ctx` first.
        let args: Vec<GenericArg> = parameters
            .iter()
            .map(|param| {
                let (member, arg_ctx) = effective(param, ctx);
                GenericArg {
                    member,
                    ctx: arg_ctx,
                }
            })
            .collect();
        Some(ResolvedType {
            res,
            def,
            ctx: TypeCtx {
                res,
                args: args.into(),
            },
        })
    }

    fn resolve_user(&self, user: &UserType, res: &'static Resolution<'static>) -> Option<Resolved> {
        match user {
            UserType::Definition(idx) => Some((res, &res[*idx])),
            UserType::Reference(idx) => {
                let type_ref = &res[*idx];
                let namespace = type_ref.namespace.as_deref().unwrap_or("");
                match &type_ref.scope {
                    ResolutionScope::Assembly(assembly) => {
                        let name = format!("{}.dll", res[*assembly].name);
                        let target = self.assemblies.resolution(&name)?;
                        self.find_type_following_forwards(target, namespace, &type_ref.name)
                    }
                    ResolutionScope::CurrentModule => {
                        self.find_type_following_forwards(res, namespace, &type_ref.name)
                    }
                    // A nested type (e.g. `Navigation/Mode`): resolve the
                    // enclosing type ref, then find the nested definition by name
                    // inside it (in the assembly the encloser lives in).
                    ResolutionScope::Nested(encloser_ref) => {
                        let encloser = UserType::Reference(*encloser_ref);
                        let (encloser_res, encloser_def) = self.resolve_user(&encloser, res)?;
                        find_nested_type(encloser_res, encloser_def, &type_ref.name)
                            .map(|def| (encloser_res, def))
                    }
                    _ => None,
                }
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

/// What a serialized field's element type classifies as. One value drives both
/// the emitted node (type name + children) and, where it differs, how a
/// collection of it is wrapped.
enum FieldKind {
    /// A primitive scalar (its AssetsTools type name), or an enum reduced to its
    /// underlying primitive.
    Primitive(String),
    /// A `string` (with its hardcoded `Array of char` children).
    String,
    /// A reference to a `UnityEngine.Object`, serialized as `PPtr<$Name>`.
    PPtr(String),
    /// A `[SerializeReference]` field, serialized as `managedReference`.
    ManagedReference,
    /// A by-value type read by name with its own serialized children (a nested
    /// serializable struct/class or a special Unity value type).
    Named {
        ty: String,
        children: Vec<TemplateField>,
    },
}

impl FieldKind {
    fn type_name(&self) -> String {
        match self {
            FieldKind::Primitive(ty) => ty.clone(),
            FieldKind::String => "string".to_string(),
            FieldKind::PPtr(name) => format!("PPtr<${name}>"),
            FieldKind::ManagedReference => "managedReference".to_string(),
            FieldKind::Named { ty, .. } => ty.clone(),
        }
    }

    fn into_children(self, version: &UnityVersion) -> Vec<TemplateField> {
        match self {
            FieldKind::Primitive(_) => Vec::new(),
            FieldKind::String => string_children(),
            FieldKind::PPtr(_) => pptr_children(version),
            FieldKind::ManagedReference => managed_reference_children(version),
            FieldKind::Named { children, .. } => children,
        }
    }

    /// Whether a collection of this kind is wrapped as a plain `vector` (size +
    /// inline data); named/managed-reference kinds keep their own type via
    /// `vector_with_type` instead.
    fn collection_is_plain_vector(&self) -> bool {
        matches!(
            self,
            FieldKind::Primitive(_) | FieldKind::String | FieldKind::PPtr(_)
        )
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

/// Find a type named `name` nested directly inside `encloser` (within `res`).
fn find_nested_type(
    res: &'static Resolution<'static>,
    encloser: &'static TypeDefinition<'static>,
    name: &str,
) -> Option<&'static TypeDefinition<'static>> {
    let encloser_ptr = std::ptr::from_ref(encloser);
    res.type_definitions.iter().find(|td| {
        td.name == name
            && td
                .encloser
                .is_some_and(|idx| std::ptr::from_ref(&res[idx]) == encloser_ptr)
    })
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
