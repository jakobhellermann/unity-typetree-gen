// TODO(ai-review): review for style and correctness
//! Owning generator that serves MonoBehaviour type trees by
//! `(assembly, namespace, class)`, so callers don't deal with dotnetdll or its
//! borrow lifetimes.
//!
//! Assemblies can be registered eagerly as raw bytes or resolved lazily via a
//! loader; either way they are parsed only on first use — a game's `Managed`
//! directory has dozens of DLLs (BCL, engine modules) but only the few that
//! actually define or are referenced by a MonoBehaviour are ever loaded.
use std::collections::HashMap;
use std::sync::Mutex;

use dotnetdll::prelude::{ReadOptions, Resolution};

use rabex::UnityVersion;

use crate::TypeTreeNode;
use crate::generator::Generator;

/// Resolves an assembly's bytes by name on demand (e.g. reading the file from a
/// game's `Managed` directory). Called the first time an assembly that wasn't
/// registered up front is needed; `None` means "no such assembly".
type Loader = Box<dyn Fn(&str) -> Option<Vec<u8>>>;

/// A set of managed assemblies that can generate MonoBehaviour type trees for a
/// fixed Unity version.
///
/// Assemblies can be registered eagerly with [`add_assembly`](Self::add_assembly)
/// or resolved lazily via a [loader](Self::with_loader) — a game's `Managed`
/// directory has dozens of DLLs (BCL, engine modules) but only the few that
/// actually define or are referenced by a MonoBehaviour are ever loaded.
///
/// Assembly bytes and parsed resolutions are leaked to obtain `'static`
/// lifetimes; this is meant for a generator built once per game environment.
pub struct AssemblyTypeTreeGenerator {
    /// Eagerly registered bytes, plus lazily loaded ones cached after first use.
    bytes: Mutex<HashMap<String, &'static [u8]>>,
    resolutions: Mutex<HashMap<String, &'static Resolution<'static>>>,
    loader: Option<Loader>,
    unity_version: UnityVersion,
}

impl AssemblyTypeTreeGenerator {
    pub fn new(unity_version: UnityVersion) -> Self {
        AssemblyTypeTreeGenerator {
            bytes: Mutex::new(HashMap::new()),
            resolutions: Mutex::new(HashMap::new()),
            loader: None,
            unity_version,
        }
    }

    /// Set a loader that resolves an assembly's bytes by name on first use, so
    /// callers don't have to read every DLL up front. Explicitly registered
    /// assemblies (and already-loaded ones) take precedence over the loader.
    pub fn with_loader(mut self, loader: impl Fn(&str) -> Option<Vec<u8>> + 'static) -> Self {
        self.loader = Some(Box::new(loader));
        self
    }

    /// Register an assembly's bytes under `assembly_name` (e.g.
    /// `Assembly-CSharp.dll`, matching `MonoScript::m_AssemblyName`). The bytes
    /// are not parsed until a type tree from this assembly is first requested.
    pub fn add_assembly(&mut self, assembly_name: String, bytes: Vec<u8>) {
        let leaked: &'static [u8] = Vec::leak(bytes);
        self.bytes.lock().unwrap().insert(assembly_name, leaked);
    }

    /// Whether `assembly_name` has been registered or already loaded. Does not
    /// consult the loader, so a lazily loadable assembly reads as `false` until
    /// it is first used.
    pub fn has_assembly(&self, assembly_name: &str) -> bool {
        self.bytes.lock().unwrap().contains_key(assembly_name)
    }

    /// Bytes for `assembly_name`: registered/cached first, otherwise via the
    /// loader (whose result is leaked and cached so it runs at most once per
    /// assembly). `None` if unknown.
    fn assembly_bytes(&self, assembly_name: &str) -> Option<&'static [u8]> {
        let mut bytes = self.bytes.lock().unwrap();
        if let Some(b) = bytes.get(assembly_name) {
            return Some(b);
        }
        let loaded = self.loader.as_ref()?(assembly_name)?;
        let leaked: &'static [u8] = Vec::leak(loaded);
        bytes.insert(assembly_name.to_owned(), leaked);
        Some(leaked)
    }

    /// Parsed resolution for `assembly_name`, parsing (and caching) it on first
    /// access. `None` if the assembly is unknown or fails to parse.
    pub(crate) fn resolution(&self, assembly_name: &str) -> Option<&'static Resolution<'static>> {
        if let Some(resolution) = self.resolutions.lock().unwrap().get(assembly_name) {
            return Some(resolution);
        }
        let bytes = self.assembly_bytes(assembly_name)?;
        // Type trees only need type/field metadata, never method IL — skipping
        // method bodies avoids the expensive part of parsing large assemblies.
        let options = ReadOptions {
            skip_method_bodies: true,
        };
        let resolution = Resolution::parse(bytes, options).ok()?;
        let leaked: &'static Resolution<'static> = Box::leak(Box::new(resolution));
        self.resolutions
            .lock()
            .unwrap()
            .insert(assembly_name.to_owned(), leaked);
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
