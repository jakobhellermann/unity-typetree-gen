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
use std::path::Path;
use std::sync::{Arc, Mutex};

use dotnetdll::prelude::{ReadOptions, Resolution};
use elsa::sync::FrozenMap;
use self_cell::self_cell;

use rabex::UnityVersion;

use crate::TypeTreeNode;
use crate::generator::Generator;

/// Resolves an assembly's bytes by name
///
/// Usually given assembly name like `Assembly-CSharp.dll` it ould read the bytes
/// from `game_Data/Managed/Assembly-CSharp.dll`.
/// Can return `std::io::ErrorKind::NotFound` to signal absence.
pub type Loader<'a> = dyn Fn(&str) -> Result<Vec<u8>, std::io::Error> + 'a;

self_cell!(
    /// An assembly's bytes together with the resolution parsed from (and borrowing) them.
    struct LoadedAssembly {
        owner: Box<[u8]>,

        #[covariant]
        dependent: Resolution,
    }
);

/// A parsed-assembly cache that generates MonoBehaviour type trees for a fixed
/// Unity version.
///
/// # Example
///
/// ```no_run
/// # use std::path::Path;
/// # use unity_typetree_gen::AssemblyTypeTreeGenerator;
/// let version = "6000.0.0f1".parse().unwrap();
/// let generator = AssemblyTypeTreeGenerator::new(version);
///
/// let managed_dir = Path::new("/path/to/Game_Data/Managed");
/// let tree = generator
///     .generate_from_dir(managed_dir, "Assembly-CSharp.dll", "MyGame", "PlayerController")?
///     .expect("assembly or type not found");
///
/// println!("{} fields", tree.children.len());
/// # Ok::<(), std::io::Error>(())
/// ```
pub struct AssemblyTypeTreeGenerator {
    /// Append-only, so references into an entry stay valid for as long as the generator lives.
    assemblies: FrozenMap<String, Box<LoadedAssembly>>,
    /// Per-assembly locks for single-flight parsing: concurrent callers for the same
    /// assembly wait for the first to parse instead of all parsing it in parallel.
    locks: Mutex<HashMap<String, Arc<Mutex<()>>>>,
    unity_version: UnityVersion,
}

impl AssemblyTypeTreeGenerator {
    pub fn new(unity_version: UnityVersion) -> Self {
        AssemblyTypeTreeGenerator {
            assemblies: FrozenMap::new(),
            locks: Mutex::new(HashMap::new()),
            unity_version,
        }
    }

    /// Parsed resolution for `assembly_name`, loading and parsing it on first
    /// access. `Ok(None)` if the loader reports the assembly is absent (`NotFound`).
    pub(crate) fn resolution<'r>(
        &'r self,
        assembly_name: &str,
        loader: &Loader,
    ) -> Result<Option<&'r Resolution<'r>>, std::io::Error> {
        if let Some(assembly) = self.assemblies.get(assembly_name) {
            return Ok(Some(assembly.borrow_dependent()));
        }
        // Single-flight: only one thread parses a given assembly. Concurrent callers for the
        // same assembly block on this per-assembly lock, then hit the cache on the re-check
        // below — avoiding a parse stampede when a parallel scan first touches an assembly.
        let assembly_lock = {
            let mut locks = self.locks.lock().unwrap();
            Arc::clone(locks.entry(assembly_name.to_owned()).or_default())
        };
        let _flight = assembly_lock.lock().unwrap();
        if let Some(assembly) = self.assemblies.get(assembly_name) {
            return Ok(Some(assembly.borrow_dependent()));
        }
        let bytes = match loader(assembly_name) {
            Ok(bytes) => bytes,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(None),
            Err(e) => return Err(e),
        };
        // Type trees only need type/field metadata, never method IL — skipping
        // method bodies avoids the expensive part of parsing large assemblies.
        #[allow(clippy::needless_update)] // improves patchability
        let options = ReadOptions {
            skip_method_bodies: true,
            lazy_method_bodies: false,
            lazy_method_signatures: true,
            ..Default::default()
        };
        let assembly = LoadedAssembly::try_new(bytes.into_boxed_slice(), |bytes| {
            Resolution::parse(bytes, options)
        })
        .map_err(|e| {
            std::io::Error::new(
                std::io::ErrorKind::InvalidData,
                format!("failed to parse {assembly_name}: {e}"),
            )
        })?;
        let assembly = self
            .assemblies
            .insert(assembly_name.to_owned(), Box::new(assembly));
        Ok(Some(assembly.borrow_dependent()))
    }

    /// Generate the type tree for `namespace.type_name` defined in
    /// `assembly_name`. The assembly, as well as possible references assemblies, are resolved on demand through `loader`.
    ///
    /// `Ok(None)` if the type can't be resolved (assembly or type absent);
    /// `Err` if the loader fails (other than `NotFound`) or an assembly fails to parse.
    pub fn generate(
        &self,
        loader: &Loader,
        assembly_name: &str,
        namespace: &str,
        type_name: &str,
    ) -> Result<Option<TypeTreeNode>, std::io::Error> {
        let Some(primary) = self.resolution(assembly_name, loader)? else {
            return Ok(None);
        };
        let children = Generator::new(self, &self.unity_version, loader)
            .read(primary, namespace, type_name)?;
        Ok(children.map(|children| crate::assemble(children, type_name)))
    }

    /// Convenience: generate using a loader that reads `<managed_dir>/<assembly_name>`.
    pub fn generate_from_dir(
        &self,
        managed_dir: &Path,
        assembly_name: &str,
        namespace: &str,
        type_name: &str,
    ) -> Result<Option<TypeTreeNode>, std::io::Error> {
        self.generate(
            &|name| std::fs::read(managed_dir.join(name)),
            assembly_name,
            namespace,
            type_name,
        )
    }

    /// Pre-load an assembly by name so it is available to [`monobehaviour_definitions`](Self::monobehaviour_definitions).
    /// Returns `true` if the assembly was found and loaded (or was already cached),
    /// `false` if the loader reports it is absent.
    pub fn load_assembly(&self, loader: &Loader, name: &str) -> Result<bool, std::io::Error> {
        self.resolution(name, loader).map(|r| r.is_some())
    }

    /// Returns a map from assembly name to the list of full type names for every
    /// type in the currently-loaded assemblies that derives (directly or
    /// transitively) from `UnityEngine.MonoBehaviour`.
    pub fn monobehaviour_definitions(
        &self,
        loader: &Loader,
    ) -> Result<BTreeMap<String, Vec<String>>, std::io::Error> {
        let g = crate::generator::Generator::new(self, &self.unity_version, loader);
        let mut defs: BTreeMap<String, Vec<String>> = BTreeMap::new();
        for asm_name in self.assemblies.keys_cloned() {
            let res = self.assemblies.get(&asm_name).unwrap().borrow_dependent();
            for td in &res.type_definitions {
                // Mono.Cecil's Types (used by the reference C# impl) only yields
                // top-level types; skip nested types to match that behaviour.
                if td.encloser.is_some() {
                    continue;
                }
                if g.derives_from_monobehaviour(res, td)? {
                    defs.entry(asm_name.clone())
                        .or_default()
                        .push(td.type_name());
                }
            }
        }
        Ok(defs)
    }
}
