# unity-typetree-gen

Generates unity typetree information from a CIL `.dll` file.

```rust
let version = "6000.0.0f1".parse().unwrap();
let generator = AssemblyTypeTreeGenerator::new(version);

let managed_dir = Path::new("/path/to/Game_Data/Managed");
let tree = generator
    .generate_from_dir(managed_dir, "Assembly-CSharp.dll", "MyGame", "PlayerController")?
    .expect("assembly or type not found");

println!("{} fields", tree.children.len());
```
