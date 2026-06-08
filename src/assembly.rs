// TODO(ai-review): review for style and correctness
//! Owning generator that serves MonoBehaviour type trees by
//! `(assembly, namespace, class)`, so callers don't deal with dotnetdll or its
//! borrow lifetimes.
//!
//! Assemblies are resolved lazily via a [`Loader`] passed to [`generate`] and
//! parsed only on first use — a game's `Managed` directory has dozens of DLLs
//! (BCL, engine modules) but only the few that actually define or are
//! referenced by a MonoBehaviour are ever loaded.
//!
//! [`generate`]: AssemblyTypeTreeGenerator::generate
use std::collections::{BTreeMap, HashMap};
use std::io;
use std::path::Path;
use std::sync::Mutex;

use dotnetdll::prelude::{ReadOptions, Resolution};

use rabex::UnityVersion;

use crate::TypeTreeNode;
use crate::generator::Generator;

/// Resolves an assembly's bytes by name (e.g. reading the file from a game's
/// `Managed` directory), passed to [`generate`](AssemblyTypeTreeGenerator::generate)
/// on each call so it can borrow a resolver without the generator owning it. A
/// `NotFound` error means "no such assembly" (treated as absent, not an error);
/// any other `io::Error` is propagated.
pub type Loader<'a> = dyn Fn(&str) -> Result<Vec<u8>, io::Error> + 'a;

/// A parsed-assembly cache that generates MonoBehaviour type trees for a fixed
/// Unity version.
///
/// Assembly bytes and parsed resolutions are leaked to obtain `'static`
/// lifetimes; this is meant for a generator built once per game environment.
pub struct AssemblyTypeTreeGenerator {
    /// Assembly bytes, loaded lazily and cached (leaked) on first use.
    bytes: Mutex<HashMap<String, &'static [u8]>>,
    resolutions: Mutex<HashMap<String, &'static Resolution<'static>>>,
    unity_version: UnityVersion,
}

impl AssemblyTypeTreeGenerator {
    pub fn new(unity_version: UnityVersion) -> Self {
        AssemblyTypeTreeGenerator {
            bytes: Mutex::new(HashMap::new()),
            resolutions: Mutex::new(HashMap::new()),
            unity_version,
        }
    }

    /// Bytes for `assembly_name`: cached first, otherwise via `loader` (whose
    /// result is leaked and cached so it runs at most once per assembly).
    /// `Ok(None)` if the loader reports the assembly is absent (`NotFound`).
    fn assembly_bytes(
        &self,
        assembly_name: &str,
        loader: &Loader,
    ) -> Result<Option<&'static [u8]>, io::Error> {
        let mut bytes = self.bytes.lock().unwrap();
        if let Some(b) = bytes.get(assembly_name) {
            return Ok(Some(b));
        }
        let loaded = match loader(assembly_name) {
            Ok(loaded) => loaded,
            Err(e) if e.kind() == io::ErrorKind::NotFound => return Ok(None),
            Err(e) => return Err(e),
        };
        let leaked: &'static [u8] = Vec::leak(loaded);
        bytes.insert(assembly_name.to_owned(), leaked);
        Ok(Some(leaked))
    }

    /// Parsed resolution for `assembly_name`, parsing (and caching) it on first
    /// access. `Ok(None)` if the assembly is absent or fails to parse.
    pub(crate) fn resolution(
        &self,
        assembly_name: &str,
        loader: &Loader,
    ) -> Result<Option<&'static Resolution<'static>>, io::Error> {
        if let Some(resolution) = self.resolutions.lock().unwrap().get(assembly_name) {
            return Ok(Some(resolution));
        }
        let Some(bytes) = self.assembly_bytes(assembly_name, loader)? else {
            return Ok(None);
        };
        // Type trees only need type/field metadata, never method IL — skipping
        // method bodies avoids the expensive part of parsing large assemblies.
        let options = ReadOptions {
            skip_method_bodies: true,
        };
        let Ok(resolution) = Resolution::parse(bytes, options) else {
            return Ok(None);
        };
        let leaked: &'static Resolution<'static> = Box::leak(Box::new(resolution));
        self.resolutions
            .lock()
            .unwrap()
            .insert(assembly_name.to_owned(), leaked);
        Ok(Some(leaked))
    }

    /// Generate the type tree for `namespace.type_name` defined in
    /// `assembly_name`. Assemblies (the primary and any cross-assembly field
    /// types) are resolved on demand through `loader`.
    ///
    /// `Ok(None)` if the type can't be resolved (assembly or type absent);
    /// `Err` if the loader itself fails (other than `NotFound`).
    pub fn generate(
        &self,
        loader: &Loader,
        assembly_name: &str,
        namespace: &str,
        type_name: &str,
    ) -> Result<Option<TypeTreeNode>, io::Error> {
        let Some(primary) = self.resolution(assembly_name, loader)? else {
            return Ok(None);
        };
        let children = Generator::new(self, &self.unity_version, loader)
            .read(primary, namespace, type_name)?;
        Ok(children.map(|children| crate::assemble(children, type_name)))
    }

    /// Pre-load an assembly by name so it is available to [`monobehaviour_definitions`].
    /// Returns `true` if the assembly was found and loaded (or was already cached),
    /// `false` if the loader reports it is absent.
    pub fn load_assembly(&self, loader: &Loader, name: &str) -> Result<bool, io::Error> {
        self.resolution(name, loader).map(|r| r.is_some())
    }

    /// Returns a map from assembly name to the list of full type names for every
    /// type in the currently-loaded assemblies that derives (directly or
    /// transitively) from `UnityEngine.MonoBehaviour`. Base types in other
    /// assemblies are resolved on demand through `loader`.
    pub fn monobehaviour_definitions(
        &self,
        loader: &Loader,
    ) -> Result<BTreeMap<String, Vec<String>>, io::Error> {
        let g = crate::generator::Generator::new(self, &self.unity_version, loader);
        let resolutions: Vec<(String, &'static Resolution<'static>)> = self
            .resolutions
            .lock()
            .unwrap()
            .iter()
            .map(|(k, v)| (k.clone(), *v))
            .collect();
        let mut defs: BTreeMap<String, Vec<String>> = BTreeMap::new();
        for (asm_name, res) in &resolutions {
            for td in &res.type_definitions {
                // Mono.Cecil's Types (used by the reference C# impl) only yields
                // top-level types; skip nested types to match that behaviour.
                if td.encloser.is_some() {
                    continue;
                }
                if g.derives_from_monobehaviour(res, td)? {
                    defs.entry(asm_name.clone()).or_default().push(td.type_name());
                }
            }
        }
        Ok(defs)
    }

    /// Convenience: generate using a loader that reads `<managed_dir>/<name>`.
    pub fn generate_from_dir(
        &self,
        managed_dir: &Path,
        assembly_name: &str,
        namespace: &str,
        type_name: &str,
    ) -> Result<Option<TypeTreeNode>, io::Error> {
        self.generate(
            &|name| std::fs::read(managed_dir.join(name)),
            assembly_name,
            namespace,
            type_name,
        )
    }
}
