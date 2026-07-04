// NmCelegansRobot.cs — Unity C# MonoBehaviour for C. elegans nematode worm robot.
// Compatible with Unity 2022.3 LTS+.
//
// Sensor channels (24 total):
//   [00-11]  12 chemoreceptors — SphereCast distance from head (celegans_s_00_chem_* .. _s_11_*)
//   [12-20]   9 mechanoreceptors — segment velocity magnitude (celegans_s_12_* .. _s_20_*)
//   [21-23]   3 vibration axes — angular velocity XYZ of root body (celegans_s_21_vibration_accel.x/y/z)
//
// Actuator channels (96 total):
//   Segment 0..23 each contribute 4 channels in order: MDL, MDR, MVL, MVR
//   MDL/MDR → dorsal-ventral drive (Z-axis ArticulationDrive)
//   MVL/MVR → lateral drive (Y-axis ArticulationDrive)
//   Channel 96 = MVULVA (no joint mapped; reserved)

using System;
using UnityEngine;

namespace NeuralMimicry
{
    /// <summary>
    /// NeuralMimicry-controlled simulation of a <i>C. elegans</i> nematode worm.
    /// The robot body is a chain of 24 articulated cylindrical segments driven by
    /// four muscle groups per segment (MDL, MDR, MVL, MVR) matching the nematode's
    /// real motor neuron layout.  Sensor modalities mirror the Webots C. elegans
    /// controller: 12 chemoreceptor distance channels, 9 mechanoreceptor velocity
    /// channels, and 3 vibration / inertial channels.
    /// </summary>
    [RequireComponent(typeof(ArticulationBody))]
    public sealed class NmCelegansRobot : NmRobotBase
    {
        // ------------------------------------------------------------------ //
        // Constants
        // ------------------------------------------------------------------ //

        private const int NumSegments      = 24;
        private const int NumChemSensors   = 12;
        private const int NumMechSensors   = 9;
        private const int NumVibSensors    = 3;
        private const int TotalSensors     = NumChemSensors + NumMechSensors + NumVibSensors; // 24
        private const int TotalActuators   = NumSegments * 4;  // 96 (channel 96 = MVULVA, unused)

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
        public override string[] SensorNames
        {
            get
            {
                var names = new string[TotalSensors];
                for (int i = 0; i < NumChemSensors; i++)
                    names[i] = $"celegans_s_{i:D2}_chem_dist";
                for (int i = 0; i < NumMechSensors; i++)
                    names[NumChemSensors + i] = $"celegans_s_{NumChemSensors + i:D2}_mech_vel";
                names[21] = "celegans_s_21_vibration_accel.x";
                names[22] = "celegans_s_21_vibration_accel.y";
                names[23] = "celegans_s_21_vibration_accel.z";
                return names;
            }
        }

        /// <inheritdoc/>
        public override string[] ActuatorNames
        {
            get
            {
                var names = new string[TotalActuators];
                string[] groups = { "MDL", "MDR", "MVL", "MVR" };
                for (int seg = 0; seg < NumSegments; seg++)
                    for (int g = 0; g < 4; g++)
                        names[seg * 4 + g] = $"celegans_o_{seg:D2}_{groups[g]}";
                return names;
            }
        }

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

            var sensors = new float[TotalSensors];

            // --- Chemoreceptors [0..11]: distance from head to nearest object.
            //     Spread 12 rays in a cone around the head's forward axis.
            Transform head = _segments[0].transform;
            for (int i = 0; i < NumChemSensors; i++)
            {
                float angle = (i / (float)NumChemSensors) * 360f;
                Vector3 dir = Quaternion.AngleAxis(angle, head.forward) * head.up;
                float dist = _chemMaxDist;
                if (Physics.SphereCast(head.position, _segmentRadius * 0.5f,
                                       dir, out RaycastHit hit,
                                       _chemMaxDist, _chemLayerMask,
                                       QueryTriggerInteraction.Ignore))
                {
                    dist = hit.distance;
                }
                // Closer = stronger signal → invert and normalise.
                sensors[i] = 1f - Mathf.Clamp01(dist / _chemMaxDist);
            }

            // --- Mechanoreceptors [12..20]: velocity of 9 evenly-spaced segments.
            float dt = Time.fixedDeltaTime;
            if (dt <= 0f) dt = 0.02f;
            for (int i = 0; i < NumMechSensors; i++)
            {
                // Map 9 receptors uniformly across 24 segments.
                int segIdx = Mathf.RoundToInt(i * (NumSegments - 1) / (float)(NumMechSensors - 1));
                segIdx = Mathf.Clamp(segIdx, 0, NumSegments - 1);

                Vector3 cur = _segments[segIdx].transform.position;
                Vector3 vel = (cur - _prevSegmentPos[segIdx]) / dt;
                _prevSegmentPos[segIdx] = cur;

                sensors[NumChemSensors + i] = Mathf.Clamp01(vel.magnitude / _mechVelMax);
            }

            // Update remaining prev positions (not sampled by mechanoreceptors).
            for (int i = 0; i < NumSegments; i++)
                _prevSegmentPos[i] = _segments[i].transform.position;

            // --- Vibration [21..23]: root angular velocity XYZ.
            Vector3 angVel = _segments[0].angularVelocity;
            sensors[21] = Mathf.Clamp01((angVel.x + _vibAngVelMax) / (2f * _vibAngVelMax));
            sensors[22] = Mathf.Clamp01((angVel.y + _vibAngVelMax) / (2f * _vibAngVelMax));
            sensors[23] = Mathf.Clamp01((angVel.z + _vibAngVelMax) / (2f * _vibAngVelMax));

            return sensors;
        }

        // ------------------------------------------------------------------ //
        // Actuator application
        // ------------------------------------------------------------------ //

        /// <inheritdoc/>
        protected override void ApplyActuators(float[] outputs)
        {
            if (_segments == null || outputs == null) return;

            // Each segment gets 4 outputs: MDL, MDR, MVL, MVR
            // MDL + MDR → dorsal-ventral balance → Z-axis drive target.
            //   net_dv = (MDR - MDL) → mapped symmetrically to [-limit, +limit]
            // MVL + MVR → lateral balance → Y-axis drive target.
            //   net_lat = (MVR - MVL) → mapped symmetrically to [-limit, +limit]

            for (int seg = 0; seg < NumSegments; seg++)
            {
                if (_segments[seg] == null) continue;

                int baseIdx = seg * 4;
                if (baseIdx + 3 >= outputs.Length) break;

                float mdl = Mathf.Clamp01(outputs[baseIdx + 0]); // MDL
                float mdr = Mathf.Clamp01(outputs[baseIdx + 1]); // MDR
                float mvl = Mathf.Clamp01(outputs[baseIdx + 2]); // MVL
                float mvr = Mathf.Clamp01(outputs[baseIdx + 3]); // MVR

                // Dorsal-ventral drive (Z): 0.5 = neutral, >0.5 = dorsal bend.
                float dvNorm = 0.5f + 0.5f * (mdr - mdl);
                DriveArticulationNorm(_segments[seg], Mathf.Clamp01(dvNorm), axis: 2);

                // Lateral drive (Y): 0.5 = neutral, >0.5 = right bend.
                float latNorm = 0.5f + 0.5f * (mvr - mvl);
                DriveArticulationNorm(_segments[seg], Mathf.Clamp01(latNorm), axis: 1);
            }
            // Channel 96 (MVULVA) is read but not applied to any joint.
        }
    }
}
