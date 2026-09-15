// Shared content adapter. Authored axes are X forward, Y left, Z up.
// Geometry is illustrative; colliders and ArticulationBody remain the physical model.
using System;
using System.Collections.Generic;
using UnityEngine;

namespace NeuralMimicry
{
    [Serializable] public sealed class NmContentObject
    {
        public string id, shape, material, cue, anchor;
        public float[] position, size, colour;
        public float yaw, radius, strength;
        public bool collision, @internal;
    }
    [Serializable] public sealed class NmContentHabitat
    {
        public string id, title;
        public float half_extent_m, wall_height;
        public string[] cues;
        public NmContentObject[] objects;
    }
    [Serializable] public sealed class NmContentProfile
    {
        public string id, kind, habitat, biology_note;
        public int sensory, output;
        public float body_length, body_height;
        public string[] sensor_names, output_names, anatomy;
        public NmContentObject[] parts;
    }
    [Serializable] public sealed class NmContentCatalogue
    {
        public int schema_version;
        public string digest;
        public NmContentHabitat[] habitats;
        public NmContentProfile[] profiles;
    }

    public sealed class NmHabitat : MonoBehaviour
    {
        private static NmContentCatalogue catalogue;
        private readonly List<Material> ownedMaterials = new List<Material>();
        public NmContentHabitat Definition { get; private set; }
        public float Radius { get; private set; }
        public static NmContentCatalogue Catalogue
        {
            get
            {
                if (catalogue != null) return catalogue;
                var asset = Resources.Load<TextAsset>("NmSimContent");
                if (asset == null) throw new InvalidOperationException("Missing generated NmSimContent; run python3 scripts/sim_content.py.");
                catalogue = JsonUtility.FromJson<NmContentCatalogue>(asset.text);
                if (catalogue.schema_version != 1) throw new InvalidOperationException("Unsupported simulation content schema.");
                return catalogue;
            }
        }
        public static NmContentProfile Profile(NmRobotBase robot)
        {
            string type = robot.GetType().Name;
            string kind = type.Contains("Celegans") ? "worm" : type.Contains("Drosophila") ? "fly" : type.Contains("Hexapod") ? "hexapod" : type.Contains("Zebrafish") ? "fish" : "nao";
            if (kind == "fly" && robot.brainConnector != null && !string.IsNullOrEmpty(robot.brainConnector.brainId) && robot.brainConnector.brainId.StartsWith("drosophila_fafb"))
                return Array.Find(Catalogue.profiles, p => p.id == "drosophila_fafb");
            return Array.Find(Catalogue.profiles, p => p.kind == kind);
        }
        public static Vector3 ToUnity(float[] p) => new Vector3(-p[1], p[2], p[0]);
        public static GameObject MakeObject(NmContentObject item, Transform parent, float scale, List<Material> materials, bool collision)
        {
            var kind = item.shape == "box" ? PrimitiveType.Cube : item.shape == "cylinder" ? PrimitiveType.Cylinder : PrimitiveType.Sphere;
            var go = GameObject.CreatePrimitive(kind);
            go.name = "nm_" + item.id;
            go.transform.SetParent(parent, false);
            go.transform.localPosition = ToUnity(item.position) * scale;
            go.transform.localRotation = Quaternion.Euler(0, -item.yaw * Mathf.Rad2Deg, 0);
            go.transform.localScale = new Vector3(item.size[1], item.size[2] * (kind == PrimitiveType.Cylinder ? .5f : 1f), item.size[0]) * scale;
            var collider = go.GetComponent<Collider>();
            if (!collision || !item.collision) { collider.enabled = false; Destroy(collider); }
            Shader shader = Shader.Find("Universal Render Pipeline/Lit") ?? Shader.Find("Standard");
            if (shader == null) throw new InvalidOperationException("A lit material shader is required for the habitat.");
            var mat = new Material(shader);
            mat.color = new Color(item.colour[0], item.colour[1], item.colour[2]);
            if (mat.HasProperty("_BaseColor")) mat.SetColor("_BaseColor", mat.color);
            if (mat.HasProperty("_Metallic")) mat.SetFloat("_Metallic", item.material == "metal" ? .5f : 0);
            if (mat.HasProperty("_Smoothness")) mat.SetFloat("_Smoothness", .35f);
            if (item.material == "emissive") { mat.EnableKeyword("_EMISSION"); mat.SetColor("_EmissionColor", mat.color); }
            if (item.material == "water")
            {
                var colour = mat.color; colour.a = .18f; mat.color = colour;
                if (mat.HasProperty("_BaseColor")) mat.SetColor("_BaseColor", colour);
                if (mat.HasProperty("_Surface")) mat.SetFloat("_Surface", 1);
                if (mat.HasProperty("_Mode")) mat.SetFloat("_Mode", 3);
                mat.SetOverrideTag("RenderType", "Transparent");
                mat.SetInt("_SrcBlend", (int)UnityEngine.Rendering.BlendMode.SrcAlpha);
                mat.SetInt("_DstBlend", (int)UnityEngine.Rendering.BlendMode.OneMinusSrcAlpha);
                mat.SetInt("_ZWrite", 0); mat.EnableKeyword("_ALPHABLEND_ON");
                mat.EnableKeyword("_SURFACE_TYPE_TRANSPARENT"); mat.renderQueue = 3000;
                go.layer = LayerMask.NameToLayer("Water") >= 0 ? LayerMask.NameToLayer("Water") : 0;
                go.GetComponent<Renderer>().shadowCastingMode = UnityEngine.Rendering.ShadowCastingMode.Off;
            }
            go.GetComponent<Renderer>().sharedMaterial = mat;
            materials.Add(mat);
            return go;
        }
        public void Build(NmContentProfile profile, float radius)
        {
            Definition = Array.Find(Catalogue.habitats, h => h.id == profile.habitat);
            Radius = radius;
            foreach (var item in Definition.objects) MakeObject(item, transform, radius, ownedMaterials, true);
            if (Array.Find(FindObjectsOfType<Light>(), l => l.type == LightType.Directional && l.enabled) == null)
            {
                var sun = new GameObject("Shared habitat key light");
                sun.transform.SetParent(transform, false);
                sun.transform.rotation = Quaternion.Euler(55, -35, 0);
                var key = sun.AddComponent<Light>();
                key.type = LightType.Directional; key.intensity = .9f; key.shadows = LightShadows.Soft;
            }
            foreach (var item in Definition.objects)
            {
                if (item.cue != "light") continue;
                var beacon = new GameObject("Habitat light beacon");
                beacon.transform.SetParent(transform, false);
                beacon.transform.localPosition = ToUnity(item.position) * radius;
                var light = beacon.AddComponent<Light>();
                light.type = LightType.Point; light.color = new Color(.99f, .86f, .52f);
                light.range = radius * 3; light.intensity = 1;
            }
            Debug.Log($"[NmHabitat] {Definition.title}; content v1 {Catalogue.digest}; {Definition.objects.Length} objects.");
        }
        // Dimensionless bounded engineering field, not a chemical/thermal solver.
        public float Sample(string cue, Vector3 point)
        {
            Vector3 local = transform.InverseTransformPoint(point) / Radius;
            float value = 0;
            foreach (var item in Definition.objects)
            {
                if (item.cue != cue || item.radius <= 0) continue;
                float d2 = (local - ToUnity(item.position)).sqrMagnitude / (item.radius * item.radius);
                value += item.strength / (1 + 4 * d2);
            }
            return Mathf.Clamp01(value);
        }
        private void OnDestroy() { foreach (var material in ownedMaterials) Destroy(material); }
    }

}
