// One class per type-tree test case. The Rust generator's output for
// each is compared against TypeTreeGeneratorAPI (AssetsTools/…) as the
// reference. Keep each case focused on one behaviour.
using System;
using System.Collections.Generic;
using UnityEngine;

namespace Fixtures
{
    // Primitive scalar fields (and their alignment).
    public class Primitives : MonoBehaviour
    {
        public bool b;
        public byte u8;
        public sbyte i8;
        public short i16;
        public ushort u16;
        public int i32;
        public uint u32;
        public long i64;
        public ulong u64;
        public float f32;
        public double f64;
        public char c;
    }

    public enum ByteEnum : byte { A, B }
    public enum SByteEnum : sbyte { A, B }
    public enum ShortEnum : short { A, B }
    public enum UShortEnum : ushort { A, B }
    public enum IntEnum { A, B }
    public enum UIntEnum : uint { A, B }
    public enum LongEnum : long { A, B }    // Int64 → not serializable, must be skipped
    public enum ULongEnum : ulong { A, B }  // UInt64 → not serializable, must be skipped

    public class Enums : MonoBehaviour
    {
        public ByteEnum be;
        public SByteEnum sbe;
        public ShortEnum she;
        public UShortEnum ushe;
        public IntEnum ie;
        public UIntEnum uie;
        public LongEnum le;
        public ULongEnum ule;
    }

    public class Strings : MonoBehaviour
    {
        public string s;
    }

    // PPtr: fields deriving from UnityEngine.Object.
    public class Pointers : MonoBehaviour
    {
        public GameObject go;
        public Transform tr;
        public Material mat;
        public Texture2D tex;
        public Pointers self; // self-ref via UEObject → allowed as PPtr
    }

    // Built-in Unity value types with hardcoded templates.
    public class SpecialTypes : MonoBehaviour
    {
        public Vector2 v2;
        public Vector3 v3;
        public Vector4 v4;
        public Quaternion q;
        public Vector2Int v2i;
        public Vector3Int v3i;
        public Color col;
        public Color32 col32;
        public Rect rect;
        public Bounds bounds;
        public BoundsInt boundsInt;
        public LayerMask mask;
        public RectOffset rectOffset;
        public AnimationCurve curve;
        public Gradient gradient;
    }

    public class Arrays : MonoBehaviour
    {
        public int[] ints;
        public float[] floats;
        public string[] strings;
        public GameObject[] gos;
        public Vector3[] verts;
    }

    public class Lists : MonoBehaviour
    {
        public List<int> ints;
        public List<string> strings;
        public List<GameObject> gos;
        public List<Vector3> verts;
    }

    [Serializable]
    public struct InnerStruct
    {
        public int a;
        public float b;
        public Vector3 v;
    }

    [Serializable]
    public class InnerClass
    {
        public string name;
        public int[] values;
    }

    public class NestedSerializable : MonoBehaviour
    {
        public InnerStruct s;
        public InnerClass c;
        public List<InnerStruct> many;
    }

    // Field selection rules.
    public class FieldVisibility : MonoBehaviour
    {
        public int serializedPublic;
        [SerializeField] private int serializedPrivate;
        private int ignoredPrivate;
        [NonSerialized] public int ignoredNonSerialized;
        public static int ignoredStatic;
        public readonly int ignoredReadonly;
        public const int ignoredConst = 5;
        [HideInInspector] public int hiddenButSerialized;
    }

    public class Inheritance : Primitives
    {
        public int extra;
    }

    public interface IPayload { }

    [Serializable]
    public class Payload : IPayload
    {
        public int amount;
        public string label;
    }

    // [SerializeReference] fields serialize as `managedReference` and append a
    // `references` ManagedReferencesRegistry to the type.
    public class ManagedRefs : MonoBehaviour
    {
        [SerializeReference] public IPayload single;
        [SerializeReference] public List<IPayload> many;
    }

    public class CustomData : ScriptableObject
    {
        public int amount;
    }

    // Fields whose type derives from UnityEngine.Object via a base class in
    // another assembly must still serialize as PPtr (cross-assembly base walk).
    public class ScriptableRefs : MonoBehaviour
    {
        public CustomData data;
        public CustomData[] many;
    }
}
