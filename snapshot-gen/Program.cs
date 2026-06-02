using System.Text.Json;
using AssetsTools.NET;
using AssetsTools.NET.Extra;
using Mono.Cecil;

// Usage: snapshot-gen <managed-dir> <unity-version> <out-dir>
// Writes <out-dir>/<Namespace>.<Name>.json — the AssetsTools.NET type
// tree (a "Base" root + the script fields) for every MonoBehaviour in
// the managed dir, flattened to {m_Type, m_Name, m_Level, m_MetaFlag}.
if (args.Length != 3)
{
    Console.Error.WriteLine("usage: snapshot-gen <managed-dir> <unity-version> <out-dir>");
    return 1;
}
string managedDir = args[0];
var unityVersion = new UnityVersion(args[1]);
string outDir = args[2];
Directory.CreateDirectory(outDir);

var resolver = new DefaultAssemblyResolver();
resolver.AddSearchDirectory(managedDir);
var readerParams = new ReaderParameters { AssemblyResolver = resolver };

var generator = new MonoCecilTempGenerator(managedDir);
var jsonOpts = new JsonSerializerOptions { WriteIndented = true };
int written = 0;

foreach (string dllPath in Directory.GetFiles(managedDir, "*.dll"))
{
    ModuleDefinition module;
    try { module = ModuleDefinition.ReadModule(dllPath, readerParams); }
    catch { continue; }

    foreach (TypeDefinition type in module.GetTypes())
    {
        if (!IsMonoBehaviour(type))
            continue;

        string ns = type.Namespace ?? "";
        List<AssetTypeTemplateField> fields = generator.Read(dllPath, ns, type.Name, unityVersion);

        var baseField = new AssetTypeTemplateField
        {
            Name = "Base",
            Type = type.Name,
            Children = fields,
        };

        var nodes = new List<Node>();
        Flatten(baseField, 0, nodes);

        string fullName = ns.Length == 0 ? type.Name : $"{ns}.{type.Name}";
        string outPath = Path.Combine(outDir, fullName + ".json");
        File.WriteAllText(outPath, JsonSerializer.Serialize(nodes, jsonOpts));
        written++;
    }
}

Console.Error.WriteLine($"wrote {written} snapshot(s) to {outDir}");
return 0;

static bool IsMonoBehaviour(TypeDefinition type)
{
    TypeReference? bt = type.BaseType;
    while (bt != null)
    {
        if (bt.FullName == "UnityEngine.MonoBehaviour")
            return true;
        try { bt = bt.Resolve()?.BaseType; }
        catch { return false; }
    }
    return false;
}

static void Flatten(AssetTypeTemplateField field, int level, List<Node> outp)
{
    outp.Add(new Node(field.Type, field.Name, level, field.IsAligned ? 0x4000 : 0));
    if (field.Children != null)
        foreach (var child in field.Children)
            Flatten(child, level + 1, outp);
}

record Node(string m_Type, string m_Name, int m_Level, int m_MetaFlag);
