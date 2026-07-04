// NmHexapodRobot.cs — Unity C# MonoBehaviour for the Freenove Big Hexapod robot.
// Compatible with Unity 2022.3 LTS+.
//
// Sensor channels (32 base + camera):
//   [00-17]  18 joint position sensors (6 legs × 3 joints: coxa, femur, tibia)
//   [18-23]   6 foot contact sensors (one per leg)
//   [24-26]   3 accelerometer channels (XYZ)
//   [27-29]   3 gyro channels (XYZ)
//   [30]      1 ultrasonic front (normalised distance)
//   [31]      1 ultrasonic rear  (normalised distance)
//   [32 .. 32+2×W×H-1]  Camera event channels: ON then OFF (default 1×1 = 2 total)
//     Base total without camera = 32; default total = 34
//
// Actuator channels (18 total):
//   [00-17]  6 legs × 3 joints (coxa, femur, tibia) = 18 joint drives

using System;
using UnityEngine;

namespace NeuralMimicry
{
    /// <summary>
    /// NeuralMimicry-controlled simulation of the Freenove Big Hexapod robot.
    /// Six legs, each with three revolute joints (coxa, femur, tibia), are driven
    /// by 18 actuator channels.  Sensor modalities mirror the Webots hexapod
    /// controller: per-joint position, foot contact, IMU, two ultrasonic rangers,
    /// and a downsampled event camera.
    /// </summary>
    [RequireComponent(typeof(ArticulationBody))]
    public sealed class NmHexapodRobot : NmRobotBase
    {
        // ------------------------------------------------------------------ //
        // Constants
        // ------------------------------------------------------------------ //

        private const int NumLegs         = 6;
        private const int JointsPerLeg    = 3;  // coxa, femur, tibia
        private const int NumLegJoints    = NumLegs * JointsPerLeg; // 18
        private const int NumFootSensors  = NumLegs;                 // 6
        private const int NumAccel        = 3;
        private const int NumGyro         = 3;
        private const int NumUltrasonics  = 2;
        private const int BaseSensors     = NumLegJoints + NumFootSensors
                                            + NumAccel + NumGyro + NumUltrasonics; // 32
        private const int TotalActuators  = NumLegJoints; // 18

        // ------------------------------------------------------------------ //
        // Inspector
        // ------------------------------------------------------------------ //

        [Header("Body Geometry")]
        [SerializeField, Tooltip("Body box half-extents (x=half-width, y=half-height, z=half-length).")]
        private Vector3 _bodyHalfExtents = new Vector3(0.15f, 0.05f, 0.10f);

        [SerializeField, Tooltip("Leg segment lengths: [0]=coxa, [1]=femur, [2]=tibia (metres).")]
        private float[] _legLengths = { 0.055f, 0.075f, 0.085f };

        [SerializeField, Tooltip("Leg segment radius for capsule colliders (metres).")]
        private float _legRadius = 0.008f;

        [Header("Joint Limits (degrees)")]
        [SerializeField, Range(10f, 90f)] private float _coxaLimit  = 60f;
        [SerializeField, Range(10f, 90f)] private float _femurLimit = 80f;
        [SerializeField, Range(10f, 90f)] private float _tibiaLimit = 100f;

        [Header("Drive Parameters")]
        [SerializeField] private float _driveStiffness = 600f;
        [SerializeField] private float _driveDamping   = 50f;

        [Header("Sensors")]
        [SerializeField, Tooltip("Maximum ultrasonic range (metres).")]
        private float _ultrasonicMaxDist = 1.5f;

        [SerializeField, Tooltip("Accelerometer normalisation divisor (m/s²).")]
        private float _accelMax = 20f;

        [SerializeField, Tooltip("Gyro normalisation divisor (rad/s).")]
        private float _gyroMax = 10f;

        [SerializeField, Tooltip("Layer mask for ultrasonic raycasts.")]
        private LayerMask _ultrasonicLayerMask = Physics.DefaultRaycastLayers;

        [Header("Camera")]
        [SerializeField, Tooltip("Event camera retina width (pixels).")]
        private int _retinaWidth = 1;

        [SerializeField, Tooltip("Event camera retina height (pixels).")]
        private int _retinaHeight = 1;

        [SerializeField, Tooltip("Pixel brightness threshold for ON/OFF classification.")]
        [Range(0f, 1f)]
        private float _onThreshold = 0.5f;

        // ------------------------------------------------------------------ //
        // Runtime state
        // ------------------------------------------------------------------ //

        // [leg, joint] ArticulationBodies.
        private ArticulationBody[,] _legJoints;

        // Foot contact accumulator.
        private float[] _footForce;

        // Camera.
        private Camera        _cam;
        private RenderTexture _camRT;
        private Texture2D     _readbackTex;

        // IMU state.
        private ArticulationBody _body;
        private Vector3          _prevVelocity;
        private Vector3          _prevBodyPos;

        // Ultrasonic origins (set during body build).
        private Transform _ultraFrontOrigin;
        private Transform _ultraRearOrigin;

        private bool _bodyBuilt;

        // Cached names.
        private string[] _sensorNamesCache;
        private string[] _actuatorNamesCache;

        // ------------------------------------------------------------------ //
        // Abstract property implementations
        // ------------------------------------------------------------------ //

        /// <inheritdoc/>
        public override string[] SensorNames
        {
            get
            {
                if (_sensorNamesCache != null) return _sensorNamesCache;
                int pixCount = _retinaWidth * _retinaHeight;
                int total = BaseSensors + 2 * pixCount;
                var names = new string[total];
                string[] legTag  = { "FL", "ML", "HL", "FR", "MR", "HR" };
                string[] jntTag  = { "coxa", "femur", "tibia" };
                int i = 0;
                for (int l = 0; l < NumLegs; l++)
                    for (int j = 0; j < JointsPerLeg; j++)
                        names[i++] = $"hex_s_{i:D2}_leg_{legTag[l]}_{jntTag[j]}_pos";
                for (int l = 0; l < NumLegs; l++)
                    names[i++] = $"hex_s_{i:D2}_foot_{legTag[l]}_contact";
                names[i++] = "hex_s_24_accel_x";
                names[i++] = "hex_s_25_accel_y";
                names[i++] = "hex_s_26_accel_z";
                names[i++] = "hex_s_27_gyro_x";
                names[i++] = "hex_s_28_gyro_y";
                names[i++] = "hex_s_29_gyro_z";
                names[i++] = "hex_s_30_ultrasonic_front";
                names[i++] = "hex_s_31_ultrasonic_rear";
                for (int p = 0; p < pixCount; p++)
                    names[i++] = $"hex_s_{i:D2}_cam_on_{p:D3}";
                for (int p = 0; p < pixCount; p++)
                    names[i++] = $"hex_s_{i:D2}_cam_off_{p:D3}";
                _sensorNamesCache = names;
                return names;
            }
        }

        /// <inheritdoc/>
        public override string[] ActuatorNames
        {
            get
            {
                if (_actuatorNamesCache != null) return _actuatorNamesCache;
                var names = new string[TotalActuators];
                string[] legTag = { "FL", "ML", "HL", "FR", "MR", "HR" };
                string[] jntTag = { "coxa", "femur", "tibia" };
                int i = 0;
                for (int l = 0; l < NumLegs; l++)
                    for (int j = 0; j < JointsPerLeg; j++)
                        names[i++] = $"hex_o_{i:D2}_leg_{legTag[l]}_{jntTag[j]}";
                _actuatorNamesCache = names;
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

        private void OnDestroy()
        {
            base.OnDestroy();
            if (_camRT      != null) { _camRT.Release(); UnityEngine.Object.Destroy(_camRT); }
            if (_readbackTex != null) UnityEngine.Object.Destroy(_readbackTex);
        }

        private void OnCollisionStay(Collision col)
        {
            if (_legJoints == null) return;
            for (int l = 0; l < NumLegs; l++)
            {
                var tibia = _legJoints[l, 2];
                if (tibia == null) continue;
                foreach (ContactPoint cp in col.contacts)
                {
                    if (cp.thisCollider.transform == tibia.transform ||
                        cp.thisCollider.transform.IsChildOf(tibia.transform))
                    {
                        _footForce[l] = Mathf.Clamp01(col.impulse.magnitude / 0.05f);
                        break;
                    }
                }
            }
        }

        // ------------------------------------------------------------------ //
        // Body construction
        // ------------------------------------------------------------------ //

        private void BuildBody()
        {
            _legJoints = new ArticulationBody[NumLegs, JointsPerLeg];
            _footForce = new float[NumLegs];

            // Root body.
            _body = GetComponent<ArticulationBody>();
            _body.mass = 0.85f; // kg
            if (GetComponent<BoxCollider>() == null)
            {
                var bc = gameObject.AddComponent<BoxCollider>();
                bc.size = _bodyHalfExtents * 2f;
            }

            // Leg attachment offsets for 3L / 3R arrangement.
            float xSign(int l) => l < 3 ? -1f : 1f;
            float zOff(int l)
            {
                int row = l % 3;
                return row == 0 ? _bodyHalfExtents.z * 0.7f :
                       row == 1 ? 0f :
                                  -_bodyHalfExtents.z * 0.7f;
            }

            float[] jntLimits = { _coxaLimit, _femurLimit, _tibiaLimit };

            for (int l = 0; l < NumLegs; l++)
            {
                Transform parent = transform;
                for (int j = 0; j < JointsPerLeg; j++)
                {
                    float len = _legLengths[Mathf.Min(j, _legLengths.Length - 1)];
                    var go = new GameObject($"Leg{l}_Jnt{j}");
                    go.transform.SetParent(parent);
                    if (j == 0)
                        go.transform.localPosition =
                            new Vector3(xSign(l) * _bodyHalfExtents.x, 0f, zOff(l));
                    else
                        go.transform.localPosition = new Vector3(0f, -len, 0f);
                    go.transform.localRotation = Quaternion.identity;

                    var col = go.AddComponent<CapsuleCollider>();
                    col.radius    = _legRadius;
                    col.height    = len;
                    col.direction = 1; // Y-axis

                    var ab = go.AddComponent<ArticulationBody>();
                    ab.mass      = 0.02f;
                    ab.jointType = ArticulationJointType.RevoluteJoint;
                    ab.xDrive    = MakeDrive(jntLimits[j]);
                    ab.linearLockX = ArticulationDofLock.LockedMotion;
                    ab.linearLockY = ArticulationDofLock.LockedMotion;
                    ab.linearLockZ = ArticulationDofLock.LockedMotion;

                    _legJoints[l, j] = ab;
                    parent = go.transform;
                }
            }

            // Ultrasonic sensor mount points.
            _ultraFrontOrigin = CreateMount("UltraFront",
                new Vector3(0f, 0f, _bodyHalfExtents.z + 0.01f));
            _ultraRearOrigin  = CreateMount("UltraRear",
                new Vector3(0f, 0f, -(_bodyHalfExtents.z + 0.01f)));

            // Head camera.
            SetupCamera();

            _prevVelocity = Vector3.zero;
            _prevBodyPos  = transform.position;
            _bodyBuilt    = true;
        }

        private ArticulationDrive MakeDrive(float limitDeg)
        {
            return new ArticulationDrive
            {
                stiffness  = _driveStiffness,
                damping    = _driveDamping,
                forceLimit = float.MaxValue,
                lowerLimit = -limitDeg,
                upperLimit =  limitDeg,
                target     = 0f
            };
        }

        private Transform CreateMount(string name, Vector3 localPos)
        {
            var go = new GameObject(name);
            go.transform.SetParent(transform);
            go.transform.localPosition = localPos;
            go.transform.localRotation = Quaternion.identity;
            return go.transform;
        }

        private void SetupCamera()
        {
            var go = new GameObject("HeadCam");
            go.transform.SetParent(transform);
            go.transform.localPosition = new Vector3(0f, _bodyHalfExtents.y,
                                                      _bodyHalfExtents.z * 0.9f);
            go.transform.localRotation = Quaternion.identity;

            _camRT = new RenderTexture(_retinaWidth, _retinaHeight, 16,
                                       RenderTextureFormat.ARGB32);
            _camRT.filterMode = FilterMode.Point;
            _camRT.Create();

            _cam                  = go.AddComponent<Camera>();
            _cam.targetTexture    = _camRT;
            _cam.fieldOfView      = 90f;
            _cam.nearClipPlane    = 0.01f;
            _cam.farClipPlane     = 5f;

            _readbackTex = new Texture2D(_retinaWidth, _retinaHeight,
                                         TextureFormat.RGB24, false);
        }

        // ------------------------------------------------------------------ //
        // Sensor collection
        // ------------------------------------------------------------------ //

        /// <inheritdoc/>
        protected override float[] CollectSensors()
        {
            if (_legJoints == null) return Array.Empty<float>();

            int pixCount = _retinaWidth * _retinaHeight;
            var sensors  = new float[BaseSensors + 2 * pixCount];
            int idx = 0;

            // --- Leg joint positions [00..17].
            for (int l = 0; l < NumLegs; l++)
                for (int j = 0; j < JointsPerLeg; j++)
                    sensors[idx++] = ReadArticulationNorm(_legJoints[l, j], 0);

            // --- Foot contacts [18..23].
            for (int l = 0; l < NumLegs; l++)
            {
                sensors[idx++] = _footForce[l];
                _footForce[l]  = Mathf.Max(0f, _footForce[l] - Time.fixedDeltaTime * 8f);
            }

            // --- Accelerometer [24..26].
            float dt = Time.fixedDeltaTime > 0f ? Time.fixedDeltaTime : 0.02f;
            Vector3 vel   = _body.velocity;
            Vector3 accel = (vel - _prevVelocity) / dt;
            _prevVelocity = vel;
            sensors[idx++] = Mathf.Clamp01((accel.x + _accelMax) / (2f * _accelMax));
            sensors[idx++] = Mathf.Clamp01((accel.y + _accelMax) / (2f * _accelMax));
            sensors[idx++] = Mathf.Clamp01((accel.z + _accelMax) / (2f * _accelMax));

            // --- Gyro [27..29].
            Vector3 angVel = _body.angularVelocity;
            sensors[idx++] = Mathf.Clamp01((angVel.x + _gyroMax) / (2f * _gyroMax));
            sensors[idx++] = Mathf.Clamp01((angVel.y + _gyroMax) / (2f * _gyroMax));
            sensors[idx++] = Mathf.Clamp01((angVel.z + _gyroMax) / (2f * _gyroMax));

            // --- Ultrasonics [30..31].
            sensors[idx++] = UltrasonicReading(_ultraFrontOrigin,  transform.forward);
            sensors[idx++] = UltrasonicReading(_ultraRearOrigin,  -transform.forward);

            // --- Camera event channels [32..32+2*pixCount-1].
            FillCameraEvents(sensors, idx, idx + pixCount, pixCount);

            return sensors;
        }

        private float UltrasonicReading(Transform origin, Vector3 dir)
        {
            if (origin == null) return 0f;
            float d = _ultrasonicMaxDist;
            if (Physics.Raycast(origin.position, dir.normalized, out RaycastHit hit,
                                _ultrasonicMaxDist, _ultrasonicLayerMask,
                                QueryTriggerInteraction.Ignore))
                d = hit.distance;
            return 1f - Mathf.Clamp01(d / _ultrasonicMaxDist);
        }

        private void FillCameraEvents(float[] sensors, int onStart, int offStart, int pixCount)
        {
            if (_cam == null || _camRT == null || pixCount == 0) return;
            _cam.Render();
            var prevRT = RenderTexture.active;
            RenderTexture.active = _camRT;
            _readbackTex.ReadPixels(new Rect(0, 0, _retinaWidth, _retinaHeight), 0, 0, false);
            _readbackTex.Apply(false);
            RenderTexture.active = prevRT;

            Color32[] pixels = _readbackTex.GetPixels32();
            for (int p = 0; p < Mathf.Min(pixCount, pixels.Length); p++)
            {
                float bright = pixels[p].r / 255f * 0.2126f +
                               pixels[p].g / 255f * 0.7152f +
                               pixels[p].b / 255f * 0.0722f;
                if (onStart  + p < sensors.Length) sensors[onStart  + p] = bright > _onThreshold ? bright : 0f;
                if (offStart + p < sensors.Length) sensors[offStart + p] = bright <= _onThreshold ? 1f - bright : 0f;
            }
        }

        // ------------------------------------------------------------------ //
        // Actuator application
        // ------------------------------------------------------------------ //

        /// <inheritdoc/>
        protected override void ApplyActuators(float[] outputs)
        {
            if (_legJoints == null || outputs == null) return;
            int idx = 0;
            for (int l = 0; l < NumLegs; l++)
                for (int j = 0; j < JointsPerLeg; j++, idx++)
                {
                    if (idx >= outputs.Length) return;
                    DriveArticulationNorm(_legJoints[l, j], outputs[idx], 0);
                }
        }
    }
}
