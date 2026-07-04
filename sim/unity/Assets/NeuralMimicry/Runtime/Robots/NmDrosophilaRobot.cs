// NmDrosophilaRobot.cs — Unity C# MonoBehaviour for Drosophila melanogaster fruit fly.
// Compatible with Unity 2022.3 LTS+.
//
// Sensor channels (418 total at default retina 12×8):
//   [000-023]  24 leg joint positions (6 legs × 4 joints: coxa, femur, tibia, tarsus)
//   [024-029]   6 foot contact forces (one per leg)
//   [030-031]   2 antennae distance sensors (L, R)
//   [032-033]   2 compact IMU channels (forward accel, yaw rate)
//   [034-417] 384 compound-eye event channels (on/off × L/R × retina_w × retina_h)
//     [034 .. 034+W*H-1]         Left ON  events
//     [034+W*H .. 034+2*W*H-1]   Left OFF events
//     [034+2*W*H .. 034+3*W*H-1] Right ON  events
//     [034+3*W*H .. 034+4*W*H-1] Right OFF events
//
// Actuator channels (48 total, named dros_o_000_* .. dros_o_047_*):
//   [000-023]  24 leg joint drives (6 legs × 4 joints)
//   [024-027]   4 wing joint drives (L flap, L rotate, R flap, R rotate)
//   [028-047]  20 channels reserved / not mapped to joints

using System;
using UnityEngine;
using UnityEngine.Rendering;

namespace NeuralMimicry
{
    /// <summary>
    /// NeuralMimicry-controlled simulation of a <i>Drosophila melanogaster</i> fruit fly.
    /// Six legs (coxa–femur–tibia–tarsus) plus two wings are driven by 28 joint
    /// actuators.  Sensor modalities match the Webots Drosophila controller:
    /// per-joint position, per-leg foot force, antennae raycasts, IMU channels,
    /// and a downsampled compound-eye event camera.
    /// </summary>
    [RequireComponent(typeof(ArticulationBody))]
    public sealed class NmDrosophilaRobot : NmRobotBase
    {
        // ------------------------------------------------------------------ //
        // Constants
        // ------------------------------------------------------------------ //

        private const int NumLegs          = 6;
        private const int JointsPerLeg     = 4;  // coxa, femur, tibia, tarsus
        private const int NumLegJoints     = NumLegs * JointsPerLeg; // 24
        private const int NumFootContacts  = NumLegs;                 // 6
        private const int NumAntennae      = 2;
        private const int NumImuSummary    = 2;
        private const int BaseSensors      = NumLegJoints + NumFootContacts + NumAntennae
                                             + NumImuSummary; // 34

        private const int TotalActuators   = 48;
        private const int NumMotorJoints   = NumLegJoints + 4; // 28

        // ------------------------------------------------------------------ //
        // Inspector
        // ------------------------------------------------------------------ //

        [Header("Eye Camera")]
        [SerializeField, Tooltip("Retina width in pixels for each compound eye camera.")]
        private int _retinaWidth = 12;

        [SerializeField, Tooltip("Retina height in pixels for each compound eye camera.")]
        private int _retinaHeight = 8;

        [SerializeField, Tooltip("Pixel brightness threshold for ON vs OFF event classification.")]
        [Range(0f, 1f)]
        private float _eyeOnThreshold = 0.5f;

        [Header("Body Geometry")]
        [SerializeField, Tooltip("Thorax half-extents (xyz metres).")]
        private Vector3 _thoraxHalfExtents = new Vector3(0.0035f, 0.002f, 0.005f);

        [SerializeField, Tooltip("Leg segment lengths: [0]=coxa, [1]=femur, [2]=tibia, [3]=tarsus.")]
        private float[] _legSegLengths = { 0.0025f, 0.0035f, 0.003f, 0.0015f };

        [Header("Joint Limits (degrees)")]
        [SerializeField] private float _coxaLimit    = 60f;
        [SerializeField] private float _femurLimit   = 80f;
        [SerializeField] private float _tibiaLimit   = 90f;
        [SerializeField] private float _tarsusLimit  = 45f;
        [SerializeField] private float _wingFlapLimit   = 120f;
        [SerializeField] private float _wingRotateLimit = 60f;

        [Header("Drive Parameters")]
        [SerializeField] private float _driveStiffness = 400f;
        [SerializeField] private float _driveDamping   = 30f;

        [Header("Sensors")]
        [SerializeField, Tooltip("Maximum antennae raycast distance (m).")]
        private float _antennaMaxDist = 0.02f;

        [SerializeField, Tooltip("Accelerometer normalisation divisor (m/s²).")]
        private float _accelMax = 20f;

        [SerializeField, Tooltip("Gyro normalisation divisor (rad/s).")]
        private float _gyroMax = 10f;

        [SerializeField, Tooltip("Layers hit by antennae raycasts.")]
        private LayerMask _antennaLayerMask = Physics.DefaultRaycastLayers;

        // ------------------------------------------------------------------ //
        // Runtime state
        // ------------------------------------------------------------------ //

        // Leg ArticulationBodies: [leg][joint] where joint 0=coxa..3=tarsus.
        private ArticulationBody[,] _legJoints;

        // Wing ArticulationBodies: [wing][joint] where joint 0=flap, 1=rotate.
        private ArticulationBody[,] _wingJoints;

        // Foot contact forces (set by collision callbacks).
        private float[] _footForce;

        // Compound eye cameras and render textures.
        private Camera   _leftEyeCam,   _rightEyeCam;
        private RenderTexture _leftEyeRT, _rightEyeRT;
        private Texture2D _eyeReadback;

        // Previous frame pixels for brightness tracking.
        private float[] _prevLeftPixels, _prevRightPixels;

        // Thorax ArticulationBody (root).
        private ArticulationBody _thorax;

        // Previous velocity for accelerometer.
        private Vector3 _prevVelocity;

        private bool _bodyBuilt;

        // Cached sensor names (computed once).
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
                int eyePixels = _retinaWidth * _retinaHeight;
                int total = BaseSensors + 4 * eyePixels;
                var names = new string[total];
                string[] legNames = { "FL", "ML", "HL", "FR", "MR", "HR" };
                string[] jointNames = { "coxa", "femur", "tibia", "tarsus" };
                int idx = 0;
                for (int l = 0; l < NumLegs; l++)
                    for (int j = 0; j < JointsPerLeg; j++)
                        names[idx++] = $"dros_s_{idx:D3}_leg_{legNames[l]}_{jointNames[j]}_pos";
                for (int l = 0; l < NumLegs; l++)
                    names[idx++] = $"dros_s_{idx:D3}_foot_{legNames[l]}_force";
                names[idx++] = "dros_s_030_antenna_L_dist";
                names[idx++] = "dros_s_031_antenna_R_dist";
                names[idx++] = "dros_s_032_imu_forward_accel";
                names[idx++] = "dros_s_033_imu_yaw_rate";
                string[] quads = { "eye_L_on", "eye_L_off", "eye_R_on", "eye_R_off" };
                for (int q = 0; q < 4; q++)
                    for (int p = 0; p < eyePixels; p++)
                        names[idx++] = $"dros_s_{idx:D3}_{quads[q]}_{p:D3}";
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
                for (int i = 0; i < TotalActuators; i++)
                    names[i] = $"dros_o_{i:D3}_joint";
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
            if (_leftEyeRT  != null) { _leftEyeRT.Release();  UnityEngine.Object.Destroy(_leftEyeRT);  }
            if (_rightEyeRT != null) { _rightEyeRT.Release(); UnityEngine.Object.Destroy(_rightEyeRT); }
            if (_eyeReadback != null) UnityEngine.Object.Destroy(_eyeReadback);
        }

        private void OnCollisionStay(Collision col)
        {
            // Identify which foot was touched by checking the collider's parent chain.
            for (int l = 0; l < NumLegs; l++)
            {
                if (_legJoints == null) break;
                ArticulationBody tarsus = _legJoints[l, 3];
                if (tarsus == null) continue;
                foreach (ContactPoint cp in col.contacts)
                {
                    if (cp.thisCollider.transform.IsChildOf(tarsus.transform) ||
                        cp.thisCollider.gameObject == tarsus.gameObject)
                    {
                        _footForce[l] = Mathf.Clamp01(col.impulse.magnitude / 0.01f);
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
            _legJoints  = new ArticulationBody[NumLegs, JointsPerLeg];
            _wingJoints = new ArticulationBody[2, 2];
            _footForce  = new float[NumLegs];

            // Root thorax ArticulationBody.
            _thorax = GetComponent<ArticulationBody>();
            if (GetComponent<BoxCollider>() == null)
            {
                var col = gameObject.AddComponent<BoxCollider>();
                col.size = _thoraxHalfExtents * 2f;
            }
            _thorax.mass = 0.00095f; // ~0.95 mg

            // Leg attachment positions (3 per side, front/mid/hind).
            Vector3[] legRoots = new Vector3[]
            {
                // Left side: FL, ML, HL
                new Vector3(-_thoraxHalfExtents.x, 0f,  _thoraxHalfExtents.z * 0.6f),
                new Vector3(-_thoraxHalfExtents.x, 0f,  0f),
                new Vector3(-_thoraxHalfExtents.x, 0f, -_thoraxHalfExtents.z * 0.6f),
                // Right side: FR, MR, HR
                new Vector3( _thoraxHalfExtents.x, 0f,  _thoraxHalfExtents.z * 0.6f),
                new Vector3( _thoraxHalfExtents.x, 0f,  0f),
                new Vector3( _thoraxHalfExtents.x, 0f, -_thoraxHalfExtents.z * 0.6f),
            };
            float[] legLimits = { _coxaLimit, _femurLimit, _tibiaLimit, _tarsusLimit };
            string[] legTag = { "FL", "ML", "HL", "FR", "MR", "HR" };
            string[] segTag = { "coxa", "femur", "tibia", "tarsus" };

            for (int l = 0; l < NumLegs; l++)
            {
                Transform parent = transform;
                for (int j = 0; j < JointsPerLeg; j++)
                {
                    var go = new GameObject($"Leg_{legTag[l]}_{segTag[j]}");
                    go.transform.SetParent(parent);
                    float len = _legSegLengths[Mathf.Min(j, _legSegLengths.Length - 1)];
                    Vector3 localPos = (j == 0) ? legRoots[l] : new Vector3(0f, -len, 0f);
                    go.transform.localPosition = localPos;
                    go.transform.localRotation = Quaternion.identity;

                    var col = go.AddComponent<CapsuleCollider>();
                    col.radius = 0.0004f;
                    col.height = len;
                    col.direction = 1; // Y-axis

                    var ab = go.AddComponent<ArticulationBody>();
                    ab.mass = 0.00002f;
                    ab.jointType = ArticulationJointType.RevoluteJoint;
                    var drive = new ArticulationDrive
                    {
                        stiffness  = _driveStiffness,
                        damping    = _driveDamping,
                        forceLimit = float.MaxValue,
                        lowerLimit = -legLimits[j],
                        upperLimit =  legLimits[j],
                        target     = 0f
                    };
                    ab.xDrive = drive;
                    ab.linearLockX = ArticulationDofLock.LockedMotion;
                    ab.linearLockY = ArticulationDofLock.LockedMotion;
                    ab.linearLockZ = ArticulationDofLock.LockedMotion;

                    _legJoints[l, j] = ab;
                    parent = go.transform;
                }
            }

            // Wings: left (index 0) and right (index 1).
            float[] wingLimits = { _wingFlapLimit, _wingRotateLimit };
            string[] wingTag = { "L", "R" };
            Vector3[] wingRoot = new Vector3[]
            {
                new Vector3(-_thoraxHalfExtents.x * 0.5f, _thoraxHalfExtents.y, 0f),
                new Vector3( _thoraxHalfExtents.x * 0.5f, _thoraxHalfExtents.y, 0f),
            };
            for (int w = 0; w < 2; w++)
            {
                Transform parent = transform;
                string[] dofTag = { "flap", "rotate" };
                for (int d = 0; d < 2; d++)
                {
                    var go = new GameObject($"Wing_{wingTag[w]}_{dofTag[d]}");
                    go.transform.SetParent(parent);
                    go.transform.localPosition = (d == 0) ? wingRoot[w] : Vector3.zero;
                    go.transform.localRotation = Quaternion.identity;

                    var col = go.AddComponent<BoxCollider>();
                    col.size = new Vector3(0.003f, 0.0002f, 0.004f);

                    var ab = go.AddComponent<ArticulationBody>();
                    ab.mass = 0.00005f;
                    ab.jointType = ArticulationJointType.RevoluteJoint;
                    var drive = new ArticulationDrive
                    {
                        stiffness  = _driveStiffness * 0.5f,
                        damping    = _driveDamping,
                        forceLimit = float.MaxValue,
                        lowerLimit = -wingLimits[d],
                        upperLimit =  wingLimits[d],
                        target     = 0f
                    };
                    ab.xDrive = drive;
                    ab.linearLockX = ArticulationDofLock.LockedMotion;
                    ab.linearLockY = ArticulationDofLock.LockedMotion;
                    ab.linearLockZ = ArticulationDofLock.LockedMotion;

                    _wingJoints[w, d] = ab;
                    parent = go.transform;
                }
            }

            // Eye cameras.
            SetupEyeCamera(ref _leftEyeCam,  ref _leftEyeRT,  "EyeL",
                           new Vector3(-0.001f, 0.001f, _thoraxHalfExtents.z));
            SetupEyeCamera(ref _rightEyeCam, ref _rightEyeRT, "EyeR",
                           new Vector3( 0.001f, 0.001f, _thoraxHalfExtents.z));

            int pixCount = _retinaWidth * _retinaHeight;
            _prevLeftPixels  = new float[pixCount];
            _prevRightPixels = new float[pixCount];
            _eyeReadback     = new Texture2D(_retinaWidth, _retinaHeight,
                                             TextureFormat.RGB24, false);

            _bodyBuilt = true;
        }

        private void SetupEyeCamera(ref Camera cam, ref RenderTexture rt,
                                    string goName, Vector3 localOffset)
        {
            var go = new GameObject(goName);
            go.transform.SetParent(transform);
            go.transform.localPosition = localOffset;
            go.transform.localRotation = Quaternion.identity;

            rt  = new RenderTexture(_retinaWidth, _retinaHeight, 16,
                                    RenderTextureFormat.ARGB32);
            rt.filterMode = FilterMode.Point;
            rt.Create();

            cam = go.AddComponent<Camera>();
            cam.targetTexture = rt;
            cam.fieldOfView   = 120f;
            cam.nearClipPlane = 0.001f;
            cam.farClipPlane  = 2f;
            cam.clearFlags    = CameraClearFlags.SolidColor;
            cam.backgroundColor = Color.black;
        }

        // ------------------------------------------------------------------ //
        // Sensor collection
        // ------------------------------------------------------------------ //

        /// <inheritdoc/>
        protected override float[] CollectSensors()
        {
            if (_legJoints == null) return Array.Empty<float>();

            int eyePixels = _retinaWidth * _retinaHeight;
            var sensors = new float[BaseSensors + 4 * eyePixels];
            int idx = 0;

            // --- Leg joint positions [0..23].
            for (int l = 0; l < NumLegs; l++)
                for (int j = 0; j < JointsPerLeg; j++)
                    sensors[idx++] = ReadArticulationNorm(_legJoints[l, j], 0);

            // --- Foot contact forces [24..29].
            for (int l = 0; l < NumLegs; l++)
            {
                sensors[idx++] = _footForce[l];
                _footForce[l] = Mathf.Max(0f, _footForce[l] - Time.fixedDeltaTime * 5f); // decay
            }

            // --- Antennae [30..31]: raycasts from head position left/right forward.
            Vector3 headPos = transform.position + transform.up * _thoraxHalfExtents.y * 2f;
            Vector3[] antennaDirs = {
                (transform.forward + transform.right  * -0.5f).normalized,
                (transform.forward + transform.right  *  0.5f).normalized
            };
            for (int a = 0; a < NumAntennae; a++)
            {
                float d = _antennaMaxDist;
                if (Physics.Raycast(headPos, antennaDirs[a], out RaycastHit hit,
                                    _antennaMaxDist, _antennaLayerMask,
                                    QueryTriggerInteraction.Ignore))
                    d = hit.distance;
                sensors[idx++] = 1f - Mathf.Clamp01(d / _antennaMaxDist);
            }

            // --- IMU summary [32..33].
            float dt = Time.fixedDeltaTime > 0f ? Time.fixedDeltaTime : 0.02f;
            Vector3 vel   = _thorax.velocity;
            Vector3 accel = (vel - _prevVelocity) / dt;
            _prevVelocity = vel;
            float forwardAccel = Vector3.Dot(transform.forward, accel);
            Vector3 angVel = _thorax.angularVelocity;
            sensors[idx++] = Mathf.Clamp01((forwardAccel + _accelMax) / (2f * _accelMax));
            sensors[idx++] = Mathf.Clamp01((angVel.y + _gyroMax) / (2f * _gyroMax));

            // --- Eye event channels [34..].
            SampleEye(_leftEyeCam,  _leftEyeRT,  _prevLeftPixels,
                      sensors, idx,
                      idx + eyePixels, eyePixels);
            SampleEye(_rightEyeCam, _rightEyeRT, _prevRightPixels,
                      sensors, idx + 2 * eyePixels,
                      idx + 3 * eyePixels, eyePixels);

            return sensors;
        }

        /// <summary>
        /// Renders one eye camera, then writes ON/OFF event channels from the
        /// frame-to-frame luminance delta for each retina pixel.
        /// </summary>
        private void SampleEye(Camera cam, RenderTexture rt, float[] prevPixels,
                               float[] sensors, int onStart, int offStart, int pixCount)
        {
            if (cam == null || rt == null) return;

            cam.Render();

            var prevRT = RenderTexture.active;
            RenderTexture.active = rt;
            _eyeReadback.ReadPixels(new Rect(0, 0, _retinaWidth, _retinaHeight), 0, 0, false);
            _eyeReadback.Apply(false);
            RenderTexture.active = prevRT;

            Color32[] pixels = _eyeReadback.GetPixels32();

            for (int p = 0; p < Mathf.Min(pixCount, pixels.Length); p++)
            {
                float brightness = (pixels[p].r / 255f * 0.2126f +
                                    pixels[p].g / 255f * 0.7152f +
                                    pixels[p].b / 255f * 0.0722f);
                float delta = brightness - prevPixels[p];
                if (Mathf.Abs(delta) < _eyeOnThreshold * 0.05f)
                    delta = 0f;
                float onEvent  = delta > 0f ? Mathf.Clamp01(delta * 4f) : 0f;
                float offEvent = delta < 0f ? Mathf.Clamp01(-delta * 4f) : 0f;
                if (onStart + p < sensors.Length)  sensors[onStart + p]  = onEvent;
                if (offStart + p < sensors.Length) sensors[offStart + p] = offEvent;
                prevPixels[p] = brightness;
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

            // --- Leg joints [0..23].
            for (int l = 0; l < NumLegs; l++)
                for (int j = 0; j < JointsPerLeg; j++, idx++)
                {
                    if (idx >= outputs.Length) return;
                    DriveArticulationNorm(_legJoints[l, j], outputs[idx], 0);
                }

            // --- Wing joints [24..27].
            for (int w = 0; w < 2; w++)
                for (int d = 0; d < 2; d++, idx++)
                {
                    if (idx >= outputs.Length) return;
                    DriveArticulationNorm(_wingJoints[w, d], outputs[idx], 0);
                }

            // Channels 28..47 are reserved and not applied.
        }
    }
}
