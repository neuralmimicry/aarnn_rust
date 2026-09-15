// NmCelegansRobot.cs — Unity C# MonoBehaviour for C. elegans nematode worm robot.
// Compatible with Unity 2022.3 LTS+.
//
// Canonical 24-channel order comes from the generated catalogue: inertial,
// touch, light/heat, taste/chemical, flow and far-field proximity channels.
// Outputs are grouped by named MDL/MDR/MVL/MVR muscles, not by segment.
// MVL24 is absent; channel 95 is the separate MVULVA readout (no body-wall joint).

using System;
using UnityEngine;

namespace NeuralMimicry
{
    /// <summary>
    /// NeuralMimicry-controlled simulation of a <i>C. elegans</i> nematode worm.
    /// The robot body is a chain of 24 articulated cylindrical segments driven by
    /// named body-wall muscle groups. Geometry and sensory fields are engineering
    /// proxies; anatomy landmarks do not assign positions to biological neurons.
    /// </summary>
    [RequireComponent(typeof(ArticulationBody))]
    public sealed class NmCelegansRobot : NmRobotBase
    {
        // ------------------------------------------------------------------ //
        // Constants
        // ------------------------------------------------------------------ //

        private const int NumSegments      = 24;
        private const int TotalSensors     = 24;
        private const int TotalActuators   = 96;

        // ------------------------------------------------------------------ //
        // Inspector
        // ------------------------------------------------------------------ //

        [Header("Body Geometry")]
        [SerializeField, Tooltip("Length of each body segment along local Z.")]
        private float _segmentLength = 0.08f;

        [SerializeField, Tooltip("Radius of each body segment cylinder.")]
        private float _segmentRadius = 0.03f;

        [SerializeField, Tooltip("Scale factor applied to the head segment (segment 0).")]
        private float _headScale = 1.35f;

        [Header("Joint Limits")]
        [SerializeField, Tooltip("Maximum dorsal-ventral angular deflection per segment (degrees).")]
        [Range(10f, 90f)]
        private float _dvLimitDeg = 40f;

        [SerializeField, Tooltip("Maximum lateral angular deflection per segment (degrees).")]
        [Range(5f, 60f)]
        private float _lateralLimitDeg = 20f;

        [Header("Drive Parameters")]
        [SerializeField, Tooltip("ArticulationDrive stiffness for all segment joints.")]
        private float _driveStiffness = 500f;

        [SerializeField, Tooltip("ArticulationDrive damping for all segment joints.")]
        private float _driveDamping = 40f;

        [Header("Chemoreception")]
        [SerializeField, Tooltip("Maximum chemoreceptor sensing distance in metres.")]
        private float _chemMaxDist = 0.5f;

        [SerializeField, Tooltip("Layer mask used for chemoreceptor SphereCast targets.")]
        private LayerMask _chemLayerMask = Physics.DefaultRaycastLayers;

        [Header("Mechanoreception")]
        [SerializeField, Tooltip("Normalisation divisor for segment velocity (m/s) → [0,1].")]
        private float _mechVelMax = 0.3f;

        [Header("Vibration Sensing")]
        [SerializeField, Tooltip("Normalisation divisor for angular velocity (rad/s) → [0,1].")]
        private float _vibAngVelMax = 5f;

        // ------------------------------------------------------------------ //
        // Runtime state
        // ------------------------------------------------------------------ //

        // Segment ArticulationBodies: index 0 = head (root), 1..23 = body chain.
        private ArticulationBody[] _segments;

        // Tracks whether Awake has already built the body hierarchy.
        private bool _bodyBuilt;

        // Previous-frame world velocities for mechanoreception (finite-difference).
        private Vector3[] _prevSegmentPos;

        // ------------------------------------------------------------------ //
        // Abstract property implementations
        // ------------------------------------------------------------------ //

        /// <inheritdoc/>
        public override string[] SensorNames => NmHabitat.Profile(this).sensor_names;
        public override string[] ActuatorNames => NmHabitat.Profile(this).output_names;
        private Vector3 previousVelocity;
        private readonly RaycastHit[] probeHits = new RaycastHit[32];

        // ------------------------------------------------------------------ //
        // MonoBehaviour lifecycle
        // ------------------------------------------------------------------ //

        private void Awake()
        {
            if (!_bodyBuilt)
                BuildBody();
        }

        // NmRobotBase.Start() called after Awake — creates client and connects.

        // ------------------------------------------------------------------ //
        // Body construction
        // ------------------------------------------------------------------ //

        /// <summary>
        /// Programmatically creates the ArticulationBody chain of 24 cylindrical
        /// segments.  Safe to call from Awake or from editor tooling.
        /// </summary>
        private void BuildBody()
        {
            _segments = new ArticulationBody[NumSegments];
            _prevSegmentPos = new Vector3[NumSegments];

            // Segment 0 is the root ArticulationBody on this GameObject.
            ArticulationBody root = GetComponent<ArticulationBody>();
            ConfigureRootArticulation(root);
            _segments[0] = root;

            Transform parent = transform;

            for (int i = 1; i < NumSegments; i++)
            {
                float scale = (i == 0) ? _headScale : 1f;
                float radius = _segmentRadius * scale;
                float length = _segmentLength * scale;

                // Create child GameObject.
                var go = new GameObject($"Seg_{i:D2}");
                go.transform.SetParent(parent);
                go.transform.localPosition = new Vector3(0f, 0f, -length);
                go.transform.localRotation = Quaternion.identity;

                // Add collider.
                var col = go.AddComponent<CapsuleCollider>();
                col.radius = radius;
                col.height = length;
                col.direction = 2; // Z-axis
                col.center = Vector3.zero;

                // Add ArticulationBody with spherical joint clamped to Z and Y.
                var ab = go.AddComponent<ArticulationBody>();
                ab.mass = 0.001f; // ~1 mg per segment
                ab.jointType = ArticulationJointType.SphericalJoint;

                // Z drive: dorsal-ventral bending.
                var zDrive = new ArticulationDrive
                {
                    stiffness  = _driveStiffness,
                    damping    = _driveDamping,
                    forceLimit = float.MaxValue,
                    lowerLimit = -_dvLimitDeg,
                    upperLimit =  _dvLimitDeg,
                    target     = 0f
                };
                ab.zDrive = zDrive;

                // Y drive: lateral bending.
                var yDrive = new ArticulationDrive
                {
                    stiffness  = _driveStiffness,
                    damping    = _driveDamping,
                    forceLimit = float.MaxValue,
                    lowerLimit = -_lateralLimitDeg,
                    upperLimit =  _lateralLimitDeg,
                    target     = 0f
                };
                ab.yDrive = yDrive;

                // Lock X drive (no torsion).
                var xDrive = new ArticulationDrive
                {
                    stiffness  = _driveStiffness * 4f,
                    damping    = _driveDamping * 2f,
                    forceLimit = float.MaxValue,
                    lowerLimit = 0f,
                    upperLimit = 0f,
                    target     = 0f
                };
                ab.xDrive = xDrive;

                // Restrict spherical DOFs to Y and Z only by disabling X swing.
                ab.linearLockX  = ArticulationDofLock.LockedMotion;
                ab.linearLockY  = ArticulationDofLock.LockedMotion;
                ab.linearLockZ  = ArticulationDofLock.LockedMotion;
                ab.swingYLock   = ArticulationDofLock.LimitedMotion;
                ab.swingZLock   = ArticulationDofLock.LimitedMotion;
                ab.twistLock    = ArticulationDofLock.LockedMotion;

                _segments[i] = ab;
                parent = go.transform;
            }

            // Initialise previous-position array at current positions.
            for (int i = 0; i < NumSegments; i++)
                _prevSegmentPos[i] = _segments[i].transform.position;

            _bodyBuilt = true;
        }

        /// <summary>Configures the root ArticulationBody (segment 0 = head).</summary>
        private void ConfigureRootArticulation(ArticulationBody root)
        {
            float headRadius = _segmentRadius * _headScale;
            float headLength = _segmentLength * _headScale;

            // Add head collider if none present.
            if (GetComponent<CapsuleCollider>() == null)
            {
                var col = gameObject.AddComponent<CapsuleCollider>();
                col.radius    = headRadius;
                col.height    = headLength;
                col.direction = 2;
                col.center    = Vector3.zero;
            }

            root.immovable = false;
            root.mass = 0.003f; // slightly heavier head
        }

        // ------------------------------------------------------------------ //
        // Sensor collection
        // ------------------------------------------------------------------ //

        /// <inheritdoc/>
        protected override float[] CollectSensors()
        {
            if (_segments == null) return Array.Empty<float>();
            var values = new float[24];
            var velocity = _segments[0].velocity;
            var acceleration = transform.InverseTransformDirection((velocity - previousVelocity) / Mathf.Max(.001f, Time.fixedDeltaTime));
            previousVelocity = velocity;
            var angular = transform.InverseTransformDirection(_segments[0].angularVelocity);
            Vector3 a = new Vector3(acceleration.z, -acceleration.x, acceleration.y);
            Vector3 g = new Vector3(angular.z, -angular.x, angular.y);
            for (int i = 0; i < 3; i++) { values[i] = Mathf.Clamp01(.5f + a[i] / 20f); values[3+i] = Mathf.Clamp01(.5f + g[i] / 20f); }
            Vector3 head = _segments[0].transform.position;
            Vector3 tail = _segments[23].transform.position;
            values[6] = Probe(head, transform.forward, _segmentRadius * 2);
            values[7] = Probe(tail, -transform.forward, _segmentRadius * 2);
            if (Habitat != null)
            {
                values[8] = Habitat.Sample("light", head - transform.right * _segmentRadius);
                values[9] = Habitat.Sample("light", head + transform.right * _segmentRadius);
                values[10] = Habitat.Sample("heat", head - transform.right * _segmentRadius);
                values[11] = Habitat.Sample("heat", head + transform.right * _segmentRadius);
                for (int i = 12; i <= 18; i++) values[i] = Habitat.Sample("chemical", (i >= 17 ? tail : head) + transform.right * ((i == 12 ? 0 : i % 2 == 0 ? 1 : -1) * _segmentRadius));
                values[19] = Habitat.Sample("flow", head); values[20] = Habitat.Sample("flow", tail);
            }
            values[21] = Probe(head, transform.forward - transform.right * .4f, _chemMaxDist);
            values[22] = Probe(head, transform.forward + transform.right * .4f, _chemMaxDist);
            values[23] = Probe(tail, -transform.forward, _chemMaxDist);
            return values;
        }
        private float Probe(Vector3 origin, Vector3 direction, float range)
        {
            int count = Physics.RaycastNonAlloc(origin, direction.normalized, probeHits, range, _chemLayerMask, QueryTriggerInteraction.Ignore);
            float distance = range;
            for (int i = 0; i < count; i++) if (!probeHits[i].transform.IsChildOf(transform)) distance = Mathf.Min(distance, probeHits[i].distance);
            return 1f - Mathf.Clamp01(distance / range);
        }

        // ------------------------------------------------------------------ //
        // Actuator application
        // ------------------------------------------------------------------ //

        /// <inheritdoc/>
        protected override void ApplyActuators(float[] outputs)
        {
            if (_segments == null || outputs == null) return;

            // Named body-wall quadrants determine opposing dorsal/ventral and
            // left/right drives; MVULVA never participates in locomotion.

            for (int seg = 1; seg < NumSegments; seg++)
            {
                if (_segments[seg] == null) continue;

                // The canonical vector is grouped by muscle labels; MVULVA is a
                // separate readout. Missing MVL24 remains absent, never another muscle.
                string[] names = ActuatorNames;
                float Muscle(string group)
                {
                    int channel = Array.FindIndex(names, n => n.EndsWith($"_{group}{seg + 1:D2}"));
                    return channel >= 0 && channel < outputs.Length ? Mathf.Clamp01(outputs[channel]) : 0f;
                }
                float mdl = Muscle("MDL"), mdr = Muscle("MDR"), mvl = Muscle("MVL"), mvr = Muscle("MVR");

                // Dorsal-ventral drive (Z): 0.5 = neutral, >0.5 = dorsal bend.
                float dvNorm = 0.5f + 0.25f * (mvl + mvr - mdl - mdr);
                DriveArticulationNorm(_segments[seg], Mathf.Clamp01(dvNorm), axis: 2);

                // Lateral drive (Y): 0.5 = neutral, >0.5 = right bend.
                float latNorm = 0.5f + 0.25f * (mdr + mvr - mdl - mvl);
                DriveArticulationNorm(_segments[seg], Mathf.Clamp01(latNorm), axis: 1);
            }
            // MVULVA is a separate readout and is not a body-wall joint.
        }
    }
}
