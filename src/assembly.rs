// TODO(ai-review): review for style and correctness
//! Owning generator that serves MonoBehaviour type trees by
//! `(assembly, namespace, class)`, so callers don't deal with dotnetdll or its
//! borrow lifetimes.
//!
//! Assemblies are registered as raw bytes and parsed lazily on first use — a
//! game's `Managed` directory has dozens of DLLs (BCL, engine modules) but only
//! the few that actually define MonoBehaviours are ever parsed.
use std::collections::HashMap;
use std::sync::Mutex;

use dotnetdll::prelude::{ReadOptions, Resolution};

use rabex::UnityVersion;

use crate::TypeTreeNode;
use crate::generator::Generator;

/// A set of managed assemblies that can generate MonoBehaviour type trees for a
/// fixed Unity version.
///
/// Assembly bytes and parsed resolutions are leaked to obtain `'static`
/// lifetimes; this is meant for a generator built once per game environment.
pub struct AssemblyTypeTreeGenerator {
    bytes: HashMap<String, &'static [u8]>,
    resolutions: Mutex<HashMap<String, &'static Resolution<'static>>>,
    unity_version: UnityVersion,
}

impl AssemblyTypeTreeGenerator {
    pub fn new(unity_version: UnityVersion) -> Self {
        AssemblyTypeTreeGenerator {
            bytes: HashMap::new(),
            resolutions: Mutex::new(HashMap::new()),
            unity_version,
        }
    }

    /// Register an assembly's bytes under `assembly_name` (e.g.
    /// `Assembly-CSharp.dll`, matching `MonoScript::m_AssemblyName`). The bytes
    /// are not parsed until a type tree from this assembly is first requested.
    pub fn add_assembly(&mut self, assembly_name: String, bytes: Vec<u8>) {
        let leaked: &'static [u8] = Vec::leak(bytes);
        self.bytes.insert(assembly_name, leaked);
    }

    pub fn has_assembly(&self, assembly_name: &str) -> bool {
        self.bytes.contains_key(assembly_name)
    }

    /// Parsed resolution for `assembly_name`, parsing (and caching) it on first
    /// access. `None` if the assembly is unknown or fails to parse.
    pub(crate) fn resolution(&self, assembly_name: &str) -> Option<&'static Resolution<'static>> {
        let mut resolutions = self.resolutions.lock().unwrap();
        if let Some(resolution) = resolutions.get(assembly_name) {
            return Some(resolution);
        }
        let bytes = *self.bytes.get(assembly_name)?;
        // Type trees only need type/field metadata, never method IL — skipping
        // method bodies avoids the expensive part of parsing large assemblies.
        let options = ReadOptions {
            skip_method_bodies: true,
        };
        let resolution = Resolution::parse(bytes, options).ok()?;
        let leaked: &'static Resolution<'static> = Box::leak(Box::new(resolution));
        resolutions.insert(assembly_name.to_owned(), leaked);
        Some(leaked)
    }

    /// Generate the flattened type tree for `namespace.type_name` defined in
    /// `assembly_name`, or `None` if that assembly is unknown / unparseable.
    /// Field types defined in other registered assemblies are resolved across
    /// assemblies (parsed on demand).
    pub fn generate(
        &self,
        assembly_name: &str,
        namespace: &str,
        type_name: &str,
    ) -> Option<TypeTreeNode> {
        let primary = self.resolution(assembly_name)?;
        let children =
            Generator::new(self, &self.unity_version).read(primary, namespace, type_name)?;
        Some(crate::assemble(children, type_name))
    }
}
