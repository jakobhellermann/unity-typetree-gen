// UnityEngine.dll facade. In real games this assembly defines (almost) nothing
// itself — it is a wall of [TypeForwardedTo] entries pointing at the module
// assemblies that hold the actual definitions. We mirror that: every type lives
// in UnityEngine.CoreModule, and we forward each one here. A MonoScript naming
// (assembly=UnityEngine.dll, class=Foo) therefore resolves only by following the
// forwarder into the module — exactly the case the generator must handle.
using System.Runtime.CompilerServices;

[assembly: TypeForwardedTo(typeof(UnityEngine.Object))]
[assembly: TypeForwardedTo(typeof(UnityEngine.Component))]
[assembly: TypeForwardedTo(typeof(UnityEngine.Behaviour))]
[assembly: TypeForwardedTo(typeof(UnityEngine.MonoBehaviour))]
[assembly: TypeForwardedTo(typeof(UnityEngine.ScriptableObject))]
[assembly: TypeForwardedTo(typeof(UnityEngine.GameObject))]
[assembly: TypeForwardedTo(typeof(UnityEngine.Transform))]
[assembly: TypeForwardedTo(typeof(UnityEngine.Material))]
[assembly: TypeForwardedTo(typeof(UnityEngine.Texture))]
[assembly: TypeForwardedTo(typeof(UnityEngine.Texture2D))]
[assembly: TypeForwardedTo(typeof(UnityEngine.Sprite))]
[assembly: TypeForwardedTo(typeof(UnityEngine.AudioClip))]
[assembly: TypeForwardedTo(typeof(UnityEngine.SerializeField))]
[assembly: TypeForwardedTo(typeof(UnityEngine.SerializeReference))]
[assembly: TypeForwardedTo(typeof(UnityEngine.HideInInspector))]
[assembly: TypeForwardedTo(typeof(UnityEngine.Vector2))]
[assembly: TypeForwardedTo(typeof(UnityEngine.Vector3))]
[assembly: TypeForwardedTo(typeof(UnityEngine.Vector4))]
[assembly: TypeForwardedTo(typeof(UnityEngine.Quaternion))]
[assembly: TypeForwardedTo(typeof(UnityEngine.Vector2Int))]
[assembly: TypeForwardedTo(typeof(UnityEngine.Vector3Int))]
[assembly: TypeForwardedTo(typeof(UnityEngine.Color))]
[assembly: TypeForwardedTo(typeof(UnityEngine.Color32))]
[assembly: TypeForwardedTo(typeof(UnityEngine.Rect))]
[assembly: TypeForwardedTo(typeof(UnityEngine.Bounds))]
[assembly: TypeForwardedTo(typeof(UnityEngine.BoundsInt))]
[assembly: TypeForwardedTo(typeof(UnityEngine.LayerMask))]
[assembly: TypeForwardedTo(typeof(UnityEngine.RectOffset))]
[assembly: TypeForwardedTo(typeof(UnityEngine.AnimationCurve))]
[assembly: TypeForwardedTo(typeof(UnityEngine.Gradient))]
[assembly: TypeForwardedTo(typeof(UnityEngine.GUIStyle))]
[assembly: TypeForwardedTo(typeof(UnityEngine.Navigation))]
[assembly: TypeForwardedTo(typeof(UnityEngine.SerializedDict<,>))]
[assembly: TypeForwardedTo(typeof(UnityEngine.ForwardedAsset))]
