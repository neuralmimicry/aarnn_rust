// Shared visual morphology attached to the existing physical articulation.
using System;
using System.Collections.Generic;
using UnityEngine;

namespace NeuralMimicry
{
    public sealed class NmRobotAppearance : MonoBehaviour
    {
        private readonly List<Material> materials = new List<Material>();
        private readonly List<GameObject> visuals = new List<GameObject>();
        private readonly List<NmContentObject> visualDefinitions = new List<NmContentObject>();
        private NmContentProfile profile;
        private ArticulationBody[] bodies;
        private float length;
        private Vector3 centre;
        public bool anatomyCutaway;
        private bool previousCutaway;
        public void Build(NmRobotBase robot)
        {
            profile = NmHabitat.Profile(robot);
            bodies = robot.GetComponentsInChildren<ArticulationBody>();
            // Native physical proportions are retained. Forward extent is measured
            // from the rig; head-rooted chains have their visual centre behind root.
            var colliders = robot.GetComponentsInChildren<Collider>();
            if (colliders.Length == 0) throw new InvalidOperationException("Robot visual requires a physical rig.");
            Bounds bounds = colliders[0].bounds;
            foreach (var c in colliders) bounds.Encapsulate(c.bounds);
            length = profile.kind == "nao" ? bounds.size.y : bounds.size.z;
            length = Mathf.Max(length, .001f);
            centre = bounds.center;
            if (profile.kind == "nao") centre.y = bounds.min.y;
            foreach (var renderer in robot.GetComponentsInChildren<Renderer>()) renderer.enabled = false;
            Rebuild();
        }
        private void Rebuild()
        {
            foreach (var go in visuals) Destroy(go);
            foreach (var mat in materials) Destroy(mat);
            visuals.Clear(); visualDefinitions.Clear(); materials.Clear();
            foreach (var item in profile.parts)
            {
                var go = NmHabitat.MakeObject(item, transform, length, materials, false);
                Vector3 inheritedScale = transform.lossyScale;
                Vector3 desiredScale = go.transform.localScale;
                go.transform.localScale = new Vector3(desiredScale.x / inheritedScale.x, desiredScale.y / inheritedScale.y, desiredScale.z / inheritedScale.z);
                go.transform.position = centre + transform.rotation * NmHabitat.ToUnity(item.position) * length;
                // Attach to the nearest existing articulated body; visual detail
                // follows joints but introduces no collision, mass or motor channel.
                Transform anchor = transform; float nearest = float.PositiveInfinity;
                foreach (var body in bodies)
                {
                    float distance = (body.transform.position - go.transform.position).sqrMagnitude;
                    if (distance < nearest) { nearest = distance; anchor = body.transform; }
                }
                go.transform.SetParent(anchor, true); visuals.Add(go); visualDefinitions.Add(item);
            }
            UpdateVisibility();
        }
        private void UpdateVisibility()
        {
            for (int i = 0; i < visuals.Count; i++)
            {
                var item = visualDefinitions[i];
                bool skin = item.id.StartsWith("cuticle_") || item.id.StartsWith("myomere_");
                visuals[i].SetActive((!item.@internal || anatomyCutaway) && !(skin && anatomyCutaway));
            }
            previousCutaway = anatomyCutaway;
        }
        private void Update() { if (profile != null && previousCutaway != anatomyCutaway) UpdateVisibility(); }
        private void OnDestroy() { foreach (var go in visuals) if (go != null) Destroy(go); foreach (var mat in materials) Destroy(mat); }
    }
}
