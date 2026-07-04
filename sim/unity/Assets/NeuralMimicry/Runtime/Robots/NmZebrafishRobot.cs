// NmZebrafishRobot.cs — Unity C# MonoBehaviour for a Danio rerio zebrafish larva.
// Compatible with Unity 2022.3 LTS+.
//
// Sensor channels (32 total):
//   [00-15]  16 lateral line mechanoreceptors (8 per side, OverlapCapsule at each body segment)
//   [16-23]   8 optical flow channels (4 quadrants × L/R eye)
//   [24-27]   4 tail joint position sensors (tail segments 0..3)
//   [28-29]   2 swim bladder pressure sensors (L, R — based on depth in "water" layer)
//   [30-31]   2 vestibular sensors (pitch acceleration, roll acceleration)
//
// Actuator channels (32 total):
//   [00-21]  11 body segments × 2 muscles (L/R) = 22 undulation channels
//             Segments 0..6 = trunk body, 7..10 = tail
//   [22-23]   2 pectoral fin channels (L, R)
//   [24-25]   2 dorsal/ventral fin channels
//   [26-31]   6 reserved

using System;
using UnityEngine;

namespace NeuralMimicry
{
    /// <summary>
    /// NeuralMimicry-controlled simulation of a <i>Danio rerio</i> zebrafish larva.
    /// An ArticulationBody chain of 11 segments (7 trunk + 4 tail) produces
    /// anguilliform undulation driven by 22 left/right muscle actuator channels.
    /// Sensor modalities mirror the Webots zebrafish controller: lateral line
    /// mechanoreception, optical flow, tail proprioception, swim-bladder depth
    /// sensing, and vestibular inertial channels.
    /// Hydrodynamic drag and lift are approximated via AddForce proportional to
    /// lateral velocity at each segment.
    /// </summary>
    [RequireComponent(typeof(ArticulationBody))]
    public sealed class NmZebrafishRobot : NmRobotBase
    {
        // ------------------------------------------------------------------ //
        // Constants
        // ------------------------------------------------------------------ //

        private const int NumBodySegments    = 7;  // trunk
        private const int NumTailSegments    = 4;  // tail
        private const int NumSegments        = NumBodySegments + NumTailSegments; // 11
        private const int NumLateralLine     = 16; // 8 per side
        private const int NumOpticalFlow     = 8;  // 4 quadrants × 2 eyes
        private const int NumTailSensors     = 4;
        private const int NumSwimBladder     = 2;
        private const int NumVestibular      = 2;
        private const int TotalSensors       = NumLateralLine + NumOpticalFlow
                                               + NumTailSensors + NumSwimBladder
                                               + NumVestibular; // 32
        private const int TotalActuators     = 32;
        private const int NumMuscleActuators = NumSegments * 2; // 22

        // ------------------------------------------------------------------ //
        // Inspector
        // ------------------------------------------------------------------ //

        [Header("Body Geometry")]
        [SerializeField, Tooltip("Base body segment half-length (Z axis, metres).")]
        private float _segHalfLen = 0.002f;

        [SerializeField, Tooltip("Base body segment radius (tapers toward tail).")]
        private float _segRadius = 0.001f;

        [SerializeField, Tooltip("Taper ratio applied per segment toward the tail tip.")]
        [Range(0.7f, 1f)]
        private float _taperRatio = 0.88f;

        [Header("Joint Limits (degrees)")]
        [SerializeField, Range(5f, 45f)] private float _trunkBendLimit = 15f;
        [SerializeField, Range(10f, 60f)] private float _tailBendLimit  = 30f;

        [Header("Drive Parameters")]
        [SerializeField] private float _driveStiffness = 200f;
        [SerializeField] private float _driveDamping   = 15f;

        [Header("Hydrodynamics")]
        [SerializeField, Tooltip("Lateral drag coefficient (N per m/s per unit area).")]
        private float _lateralDragCoeff = 0.05f;

        [SerializeField, Tooltip("Lift coefficient scaling (N per m/s²).")]
        private float _liftCoeff = 0.02f;

        [Header("Lateral Line")]
        [SerializeField, Tooltip("Detection sphere radius for lateral line OverlapCapsule.")]
        private float _lateralLineRadius = 0.003f;

        [SerializeField, Tooltip("Layers detected by the lateral line.")]
        private LayerMask _lateralLineMask = Physics.DefaultRaycastLayers;

        [Header("Optical Flow")]
        [SerializeField] private int _retinaWidth  = 4;
        [SerializeField] private int _retinaHeight = 4;

        [Header("Swim Bladder")]
        [SerializeField, Tooltip("Unity layer name for the 'water' volume.")]
        private string _waterLayerName = "Water";

        [SerializeField, Tooltip("World Y at water surface (metres).")]
        private float _waterSurfaceY = 0f;

        [SerializeField, Tooltip("Depth range for swim bladder normalisation (metres).")]
        private float _swimBladderDepthRange = 0.05f;

        [Header("Vestibular")]
        [SerializeField] private float _vestibularAngAccelMax = 30f;

        // ------------------------------------------------------------------ //
        // Runtime state
        // ------------------------------------------------------------------ //

        private ArticulationBody[] _segments;  // length = NumSegments

        // Optical flow cameras.
        private Camera        _leftEyeCam,  _rightEyeCam;
        private RenderTexture _leftEyeRT,   _rightEyeRT;
        private Texture2D     _readbackTex;
        private float[]       _prevLeftPixels, _prevRightPixels;

        // Vestibular.
        private Vector3 _prevAngVel;

        // Water layer mask (cached).
        private int _waterLayerMask;

        private bool _bodyBuilt;

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
                var n = new string[TotalSensors];
                int i = 0;
                for (int s = 0; s < 8; s++) n[i++] = $"zf_s_{i:D2}_ll_L_seg{s}";
                for (int s = 0; s < 8; s++) n[i++] = $"zf_s_{i:D2}_ll_R_seg{s}";
                for (int q = 0; q < 4; q++) n[i++] = $"zf_s_{i:D2}_of_L_q{q}";
                for (int q = 0; q < 4; q++) n[i++] = $"zf_s_{i:D2}_of_R_q{q}";
                for (int t = 0; t < 4; t++) n[i++] = $"zf_s_{i:D2}_tail_seg{t}_pos";
                n[i++] = "zf_s_28_swimbladder_L";
                n[i++] = "zf_s_29_swimbladder_R";
                n[i++] = "zf_s_30_vestibular_pitch_acc";
                n[i++] = "zf_s_31_vestibular_roll_acc";
                _sensorNamesCache = n;
                return n;
            }
        }

        /// <inheritdoc/>
        public override string[] ActuatorNames
        {
            get
            {
                if (_actuatorNamesCache != null) return _actuatorNamesCache;
                var n = new string[TotalActuators];
                for (int seg = 0; seg < NumSegments; seg++)
                {
                    n[seg * 2 + 0] = $"zf_o_{seg * 2:D2}_seg{seg:D2}_L";
                    n[seg * 2 + 1] = $"zf_o_{seg * 2 + 1:D2}_seg{seg:D2}_R";
                }
                n[22] = "zf_o_22_fin_pect_L";
                n[23] = "zf_o_23_fin_pect_R";
                n[24] = "zf_o_24_fin_dorsal";
                n[25] = "zf_o_25_fin_ventral";
                for (int r = 26; r < 32; r++) n[r] = $"zf_o_{r:D2}_reserved";
                _actuatorNamesCache = n;
                return n;
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
            if (_leftEyeRT   != null) { _leftEyeRT.Release();  UnityEngine.Object.Destroy(_leftEyeRT);  }
            if (_rightEyeRT  != null) { _rightEyeRT.Release(); UnityEngine.Object.Destroy(_rightEyeRT); }
            if (_readbackTex != null) UnityEngine.Object.Destroy(_readbackTex);
        }

        // ------------------------------------------------------------------ //
        // Body construction
        // ------------------------------------------------------------------ //

        private void BuildBody()
        {
            _segments = new ArticulationBody[NumSegments];

            // Root segment (head).
            ArticulationBody root = GetComponent<ArticulationBody>();
            root.mass = 0.0003f; // ~0.3 mg larva
            if (GetComponent<CapsuleCollider>() == null)
            {
                var hc = gameObject.AddComponent<CapsuleCollider>();
                hc.radius    = _segRadius * 2f;
                hc.height    = _segHalfLen * 4f;
                hc.direction = 2;
            }
            _segments[0] = root;

            Transform parent = transform;
            float radius = _segRadius;
            float halfLen = _segHalfLen;

            for (int i = 1; i < NumSegments; i++)
            {
                bool isTail = i >= NumBodySegments;
                radius   *= _taperRatio;
                halfLen  *= (isTail ? _taperRatio * 0.95f : 1f);
                float limitDeg = isTail ? _tailBendLimit : _trunkBendLimit;

                var go = new GameObject($"Seg{i:D2}");
                go.transform.SetParent(parent);
                go.transform.localPosition = new Vector3(0f, 0f, -(halfLen * 2f));
                go.transform.localRotation = Quaternion.identity;

                var col = go.AddComponent<CapsuleCollider>();
                col.radius    = radius;
                col.height    = halfLen * 2f;
                col.direction = 2;

                var ab = go.AddComponent<ArticulationBody>();
                ab.mass      = 0.00002f;
                ab.jointType = ArticulationJointType.SphericalJoint;

                // Y-axis: lateral undulation (primary swimming DOF).
                ab.yDrive = new ArticulationDrive
                {
                    stiffness  = _driveStiffness,
                    damping    = _driveDamping,
                    forceLimit = float.MaxValue,
                    lowerLimit = -limitDeg,
                    upperLimit =  limitDeg,
                    target     = 0f
                };
                // Z-axis: dorsal-ventral (secondary).
                ab.zDrive = new ArticulationDrive
                {
                    stiffness  = _driveStiffness * 0.5f,
                    damping    = _driveDamping,
                    forceLimit = float.MaxValue,
                    lowerLimit = -limitDeg * 0.5f,
                    upperLimit =  limitDeg * 0.5f,
                    target     = 0f
                };
                // Lock X (no axial torsion).
                ab.xDrive = new ArticulationDrive
                {
                    stiffness  = _driveStiffness * 4f,
                    damping    = _driveDamping * 2f,
                    forceLimit = float.MaxValue,
                    lowerLimit = 0f,
                    upperLimit = 0f,
                    target     = 0f
                };
                ab.linearLockX = ArticulationDofLock.LockedMotion;
                ab.linearLockY = ArticulationDofLock.LockedMotion;
                ab.linearLockZ = ArticulationDofLock.LockedMotion;
                ab.swingYLock  = ArticulationDofLock.LimitedMotion;
                ab.swingZLock  = ArticulationDofLock.LimitedMotion;
                ab.twistLock   = ArticulationDofLock.LockedMotion;

                _segments[i] = ab;
                parent = go.transform;
            }

            // Eye cameras for optical flow.
            SetupEyeCamera(ref _leftEyeCam,  ref _leftEyeRT,  "EyeL",
                           new Vector3(-_segRadius * 3f, 0f, _segHalfLen * 2f));
            SetupEyeCamera(ref _rightEyeCam, ref _rightEyeRT, "EyeR",
                           new Vector3( _segRadius * 3f, 0f, _segHalfLen * 2f));

            int pixCount = _retinaWidth * _retinaHeight;
            _prevLeftPixels  = new float[pixCount];
            _prevRightPixels = new float[pixCount];
            _readbackTex     = new Texture2D(_retinaWidth, _retinaHeight,
                                              TextureFormat.RGB24, false);

            // Resolve water layer mask.
            int waterLayer = LayerMask.NameToLayer(_waterLayerName);
            _waterLayerMask = waterLayer >= 0 ? (1 << waterLayer) : 0;

            _prevAngVel = Vector3.zero;
            _bodyBuilt  = true;
        }

        private void SetupEyeCamera(ref Camera cam, ref RenderTexture rt,
                                    string goName, Vector3 localOffset)
        {
            var go = new GameObject(goName);
            go.transform.SetParent(transform);
            go.transform.localPosition = localOffset;
            go.transform.localRotation = Quaternion.identity;

            rt = new RenderTexture(_retinaWidth, _retinaHeight, 16, RenderTextureFormat.ARGB32);
            rt.filterMode = FilterMode.Point;
            rt.Create();

            cam                  = go.AddComponent<Camera>();
            cam.targetTexture    = rt;
            cam.fieldOfView      = 130f; // wide-angle larval eye
            cam.nearClipPlane    = 0.0005f;
            cam.farClipPlane     = 0.5f;
        }

        // ------------------------------------------------------------------ //
        // Physics: hydrodynamic forces applied each FixedUpdate.
        // ------------------------------------------------------------------ //

        private void FixedUpdate()
        {
            // Apply hydrodynamic drag/lift per segment.
            // This is called every physics frame by Unity; NmRobotBase.FixedUpdate
            // is also called (base class is separate — no override needed here since
            // ArticulationBody handles internal physics; we add forces on top).
            if (_segments == null) return;
            for (int i = 0; i < NumSegments; i++)
            {
                ArticulationBody ab = _segments[i];
                if (ab == null) continue;

                // Lateral velocity component (perpendicular to segment long axis = local X).
                Vector3 worldLateralVel = Vector3.ProjectOnPlane(
                    ab.velocity,
                    ab.transform.forward);

                // Drag opposing lateral motion.
                Vector3 dragForce = -_lateralDragCoeff * worldLateralVel;
                ab.AddForce(dragForce, ForceMode.Force);

                // Lift: vortex shedding approximation — cross product of vel and forward.
                Vector3 lift = Vector3.Cross(ab.velocity, ab.transform.forward) * _liftCoeff;
                ab.AddForce(lift, ForceMode.Force);
            }
        }

        // ------------------------------------------------------------------ //
        // Sensor collection
        // ------------------------------------------------------------------ //

        /// <inheritdoc/>
        protected override float[] CollectSensors()
        {
            if (_segments == null) return Array.Empty<float>();

            var sensors = new float[TotalSensors];
            int idx = 0;

            // --- Lateral line [00-15]: OverlapCapsule at each side of 8 mapped segments.
            //     Map 8 samples per side uniformly across NumSegments.
            for (int side = 0; side < 2; side++) // 0=left, 1=right
            {
                float sideSign = side == 0 ? -1f : 1f;
                for (int k = 0; k < 8; k++, idx++)
                {
                    int segIdx = Mathf.RoundToInt(k * (NumSegments - 1) / 7f);
                    segIdx = Mathf.Clamp(segIdx, 0, NumSegments - 1);
                    ArticulationBody ab = _segments[segIdx];
                    if (ab == null) { sensors[idx] = 0f; continue; }

                    Vector3 sideOffset = ab.transform.right * sideSign * _segRadius * 2f;
                    Vector3 point1 = ab.transform.position + sideOffset + ab.transform.forward * _segHalfLen;
                    Vector3 point2 = ab.transform.position + sideOffset - ab.transform.forward * _segHalfLen;

                    Collider[] hits = Physics.OverlapCapsule(point1, point2,
                                                             _lateralLineRadius,
                                                             _lateralLineMask,
                                                             QueryTriggerInteraction.Ignore);
                    // Signal strength proportional to number of nearby objects (clamped).
                    sensors[idx] = Mathf.Clamp01(hits.Length / 3f);
                }
            }

            // --- Optical flow [16-23]: frame-derivative brightness in 4 quadrants per eye.
            SampleOpticalFlow(_leftEyeCam,  _leftEyeRT,  _prevLeftPixels,  sensors, idx);
            idx += 4;
            SampleOpticalFlow(_rightEyeCam, _rightEyeRT, _prevRightPixels, sensors, idx);
            idx += 4;

            // --- Tail joint positions [24-27]: tail segments 0..3 (segments 7..10).
            for (int t = 0; t < NumTailSensors; t++, idx++)
            {
                int segIdx = NumBodySegments + t;
                sensors[idx] = segIdx < NumSegments
                    ? ReadArticulationNorm(_segments[segIdx], axis: 1)
                    : 0f;
            }

            // --- Swim bladder [28-29]: depth-based pressure.
            //     Each side samples a slightly offset position.
            float depth = _waterSurfaceY - transform.position.y;
            float bladderNorm = Mathf.Clamp01(depth / _swimBladderDepthRange);
            sensors[idx++] = bladderNorm; // L
            sensors[idx++] = bladderNorm; // R (symmetric in larva)

            // --- Vestibular [30-31]: angular acceleration pitch and roll.
            Vector3 angVel = _segments[0].angularVelocity;
            Vector3 angAcc = (angVel - _prevAngVel) / (Time.fixedDeltaTime > 0f ? Time.fixedDeltaTime : 0.02f);
            _prevAngVel = angVel;
            sensors[idx++] = Mathf.Clamp01((angAcc.x + _vestibularAngAccelMax) / (2f * _vestibularAngAccelMax)); // pitch
            sensors[idx++] = Mathf.Clamp01((angAcc.z + _vestibularAngAccelMax) / (2f * _vestibularAngAccelMax)); // roll

            return sensors;
        }

        /// <summary>
        /// Samples an eye camera and computes temporal derivative brightness across
        /// 4 quadrants, filling 4 sensor channels (one per quadrant).
        /// </summary>
        private void SampleOpticalFlow(Camera cam, RenderTexture rt, float[] prevPixels,
                                       float[] sensors, int startIdx)
        {
            if (cam == null || rt == null)
            {
                for (int q = 0; q < 4; q++)
                    if (startIdx + q < sensors.Length) sensors[startIdx + q] = 0f;
                return;
            }

            cam.Render();
            var prevRT = RenderTexture.active;
            RenderTexture.active = rt;
            _readbackTex.ReadPixels(new Rect(0, 0, _retinaWidth, _retinaHeight), 0, 0, false);
            _readbackTex.Apply(false);
            RenderTexture.active = prevRT;

            Color32[] pixels = _readbackTex.GetPixels32();
            int pixCount = _retinaWidth * _retinaHeight;
            int halfW = _retinaWidth  / 2;
            int halfH = _retinaHeight / 2;

            // 4 quadrants: TL=0, TR=1, BL=2, BR=3
            float[] quadSum  = new float[4];
            float[] quadCnt  = new float[4];
            for (int p = 0; p < Mathf.Min(pixCount, pixels.Length); p++)
            {
                int px = p % _retinaWidth;
                int py = p / _retinaWidth;
                int q  = (py < halfH ? 0 : 2) + (px >= halfW ? 1 : 0);

                float bright = pixels[p].r / 255f * 0.2126f +
                               pixels[p].g / 255f * 0.7152f +
                               pixels[p].b / 255f * 0.0722f;
                float delta = Mathf.Abs(bright - (p < prevPixels.Length ? prevPixels[p] : 0f));
                quadSum[q] += delta;
                quadCnt[q] += 1f;
                if (p < prevPixels.Length) prevPixels[p] = bright;
            }

            for (int q = 0; q < 4; q++)
            {
                float flow = quadCnt[q] > 0f ? quadSum[q] / quadCnt[q] : 0f;
                if (startIdx + q < sensors.Length)
                    sensors[startIdx + q] = Mathf.Clamp01(flow * 4f); // scale up small deltas
            }
        }

        // ------------------------------------------------------------------ //
        // Actuator application
        // ------------------------------------------------------------------ //

        /// <inheritdoc/>
        protected override void ApplyActuators(float[] outputs)
        {
            if (_segments == null || outputs == null) return;

            // --- Body segment undulation [00-21]: L/R muscle pair per segment.
            //     Left activation (>0.5) bends the segment left (negative Y).
            //     Right activation (>0.5) bends the segment right (positive Y).
            //     Net lateral drive = (R - L) mapped to Y-axis drive target.
            for (int seg = 0; seg < NumSegments; seg++)
            {
                int baseIdx = seg * 2;
                if (baseIdx + 1 >= outputs.Length || _segments[seg] == null) break;

                float muscleL = Mathf.Clamp01(outputs[baseIdx + 0]);
                float muscleR = Mathf.Clamp01(outputs[baseIdx + 1]);

                // Net lateral drive: 0.5 = neutral, deviation creates bend.
                float latNorm = 0.5f + 0.5f * (muscleR - muscleL);
                DriveArticulationNorm(_segments[seg], latNorm, axis: 1);
            }

            // --- Pectoral fins [22-23]: applied as Z-axis (dorsal-ventral) on seg 1 and 2.
            if (22 < outputs.Length && NumBodySegments > 1 && _segments[1] != null)
                DriveArticulationNorm(_segments[1], outputs[22], axis: 2);
            if (23 < outputs.Length && NumBodySegments > 2 && _segments[2] != null)
                DriveArticulationNorm(_segments[2], outputs[23], axis: 2);

            // --- Dorsal/ventral fins [24-25]: modulate Z drive on mid-body segments.
            if (24 < outputs.Length && NumBodySegments > 3 && _segments[3] != null)
                DriveArticulationNorm(_segments[3], outputs[24], axis: 2);
            if (25 < outputs.Length && NumBodySegments > 4 && _segments[4] != null)
                DriveArticulationNorm(_segments[4], outputs[25], axis: 2);

            // Channels 26..31: reserved, not applied.
        }
    }
}
