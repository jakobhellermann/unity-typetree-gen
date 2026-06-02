// Minimal hand-written UnityEngine stubs — just enough names, base
// chains and attributes for the fixture classes to compile and for the
// type-tree generators to classify fields (special types are matched by
// full name, PPtr by deriving from UnityEngine.Object). NOT real Unity
// code.
namespace UnityEngine
{
    public class Object { }
    public class Component : Object { }
    public class Behaviour : Component { }
    public class MonoBehaviour : Behaviour { }
    public class ScriptableObject : Object { }

    public class GameObject : Object { }
    public class Transform : Component { }
    public class Material : Object { }
    public class Texture : Object { }
    public class Texture2D : Texture { }
    public class Sprite : Object { }
    public class AudioClip : Object { }

    public sealed class SerializeField : System.Attribute { }
    public sealed class SerializeReference : System.Attribute { }
    public sealed class HideInInspector : System.Attribute { }

    public struct Vector2 { public float x, y; }
    public struct Vector3 { public float x, y, z; }
    public struct Vector4 { public float x, y, z, w; }
    public struct Quaternion { public float x, y, z, w; }
    public struct Vector2Int { public int x, y; }
    public struct Vector3Int { public int x, y, z; }
    public struct Color { public float r, g, b, a; }
    public struct Color32 { public byte r, g, b, a; }
    public struct Rect { public float x, y, width, height; }
    public struct Bounds { public Vector3 center, extents; }
    public struct BoundsInt { public Vector3Int position, size; }
    public struct LayerMask { public int value; }

    public class RectOffset { public int left, right, top, bottom; }
    public class AnimationCurve { }
    public class Gradient { }
}
