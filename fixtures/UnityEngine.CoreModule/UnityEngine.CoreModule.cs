// The real UnityEngine type definitions. In a shipped game these live in the
// module assemblies (UnityEngine.CoreModule.dll etc.), and UnityEngine.dll only
// forwards to them via [TypeForwardedTo]. Mirroring that here lets us exercise
// the generator's type-forwarder following for a *top-level* type whose
// MonoScript names UnityEngine.dll but whose definition lives in a module.
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

    // A struct with a nested enum, in a different assembly than the fixtures.
    // A field typed `Navigation.Mode` is a cross-assembly reference to a *nested*
    // type, which the generator must resolve via the enclosing type ref.
    // (Mirrors UnityEngine.UI.Navigation.Mode.)
    public struct Navigation
    {
        public enum Mode { Automatic, Horizontal, Vertical, None }
        public Mode mode;
    }

    // A type whose definition lives in this module assembly while UnityEngine.dll
    // forwards its name. A MonoScript pointing at
    // (assembly=UnityEngine.dll, class=ForwardedAsset) must follow the forwarder
    // to find these fields. (Mirrors real types like FontAsset / ThemeStyleSheet,
    // which a MonoScript names under UnityEngine.dll but whose definitions live in
    // module assemblies.) It derives from MonoBehaviour so snapshot-gen emits a
    // reference snapshot for it.
    public class ForwardedAsset : MonoBehaviour
    {
        public int amount;
        public string label;
        public Vector3 offset;
    }
}
