# unity-typetree-gen

Generates unity typetree information from a CIL `.dll` file. Adapted from [AssetsTools.NET MonoCecilTempGenerator](https://github.com/nesrak1/AssetsTools.NET/blob/main/AssetsTools.NET.MonoCecil/MonoCecilTempGenerator.cs).

```rust
let version = "6000.0.0f1".parse().unwrap();
let generator = AssemblyTypeTreeGenerator::new(version);

let managed_dir = Path::new("/path/to/Game_Data/Managed");
let tree = generator
    .generate_from_dir(managed_dir, "Assembly-CSharp.dll", "MyGame", "PlayerController")?
    .expect("assembly or type not found");

println!("{}", tree.dump());
```
