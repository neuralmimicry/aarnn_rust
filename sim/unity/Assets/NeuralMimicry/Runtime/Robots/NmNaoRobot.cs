// NmNaoRobot.cs — Unity C# MonoBehaviour for the NAO V6 humanoid robot.
// Compatible with Unity 2022.3 LTS+.
//
// Sensor channels (~250 total at default retina 8×6):
//   [00-01]   2 sonar (L, R): SphereCast from chest, max 2.55 m
//   [02-04]   3 accelerometer XYZ
//   [05-06]   2 gyro XY
//   [07-09]   3 GPS XYZ (world position, metre-scale, normalised per axis)
//   [10-12]   3 inertial (roll, pitch, yaw) normalised to [0,1]
//   [13-20]   8 foot pressure (L4 + R4 contact points at toe/heel corners)
//   [21-22]   2 bumpers (chest L, chest R)
//   [23-48]   26 joint position channels (one per motor joint)
//   [49-57]    9 joint-velocity channels
//   [58 .. 58 + 4*W*H - 1]  head camera event channels (on/off × L/R, 8×6 default = 192)
//     [58 .. 58+W*H-1]         left ON
//     [58+W*H .. 58+2*W*H-1]   left OFF
//     [58+2*W*H .. 58+3*W*H-1] right ON
//     [58+3*W*H .. 58+4*W*H-1] right OFF
//
// Actuator channels (40 total):
//   [00-01]   shoulder pitch L, R
//   [02-03]   shoulder roll  L, R
//   [04-05]   elbow yaw      L, R
//   [06-07]   elbow roll     L, R
//   [08-09]   wrist yaw      L, R
//   [10-11]   hand           L, R
//   [12-13]   hip yaw-pitch  L, R
//   [14-15]   hip roll       L, R
//   [16-17]   hip pitch      L, R
//   [18-19]   knee pitch     L, R
//   [20-21]   ankle pitch    L, R
//   [22-23]   ankle roll     L, R
//   [24-31]   head pitch, head yaw, chest RGB LED (3), foot L LED, foot R LED, head LED = 8
//   [32-39]   reserved

using System;
using UnityEngine;

namespace NeuralMimicry
{
    /// <summary>
    /// NeuralMimicry-controlled simulation of the NAO V6 humanoid robot.
    /// A full ArticulationBody hierarchy replicates the NAO's 25 DOF (arms, legs,
    /// head).  Sensor modalities mirror the Webots NAO NN controller: sonar, IMU,
    /// GPS, foot pressure, bumpers, and a downsampled head-camera event stream.
    /// Actuators cover 26 joint drives plus 8 LED channels (mapped from the 40
    /// neural output channels).
    /// </summary>
    [RequireComponent(typeof(ArticulationBody))]
    public sealed class NmNaoRobot : NmRobotBase
    {
        // ------------------------------------------------------------------ //
        // Constants
        // ------------------------------------------------------------------ //

        private const int BaseSensors    = 58;
        private const int TotalActuators = 40;
        private const int NumJointVelocitySensors = 9;

        // Joint indices into _joints[] array (matches ActuatorNames order).
        private enum JointId
        {
            ShoulderPitchL, ShoulderPitchR,
            ShoulderRollL,  ShoulderRollR,
            ElbowYawL,      ElbowYawR,
            ElbowRollL,     ElbowRollR,
            WristYawL,      WristYawR,
            HandL,          HandR,
            HipYawPitchL,   HipYawPitchR,
            HipRollL,       HipRollR,
            HipPitchL,      HipPitchR,
            KneePitchL,     KneePitchR,
            AnklePitchL,    AnklePitchR,
            AnkleRollL,     AnkleRollR,
            HeadPitch,      HeadYaw,
            Count // 26 motor joints
        }
        private const int NumMotorJoints = (int)JointId.Count; // 26

        // ------------------------------------------------------------------ //
        // Inspector
        // ------------------------------------------------------------------ //

        [Header("Body Dimensions (NAO V6 approximate)")]
        [SerializeField] private float _torsoHeight    = 0.2f;
        [SerializeField] private float _torsoWidth     = 0.11f;
        [SerializeField] private float _torsoDepth     = 0.08f;
        [SerializeField] private float _upperArmLen    = 0.09f;
        [SerializeField] private float _lowerArmLen    = 0.075f;
        [SerializeField] private float _thighLen       = 0.1f;
        [SerializeField] private float _shinLen        = 0.1025f;
        [SerializeField] private float _footLen        = 0.16f;
        [SerializeField] private float _footWidth      = 0.08f;
        [SerializeField] private float _headRadius     = 0.05f;
        [SerializeField] private float _limbRadius     = 0.018f;

        [Header("Drive Parameters")]
        [SerializeField] private float _driveStiffness = 800f;
        [SerializeField] private float _driveDamping   = 60f;

        [Header("Sonar")]
        [SerializeField, Tooltip("Max sonar range in metres.")]
        private float _sonarMaxDist = 2.55f;
        [SerializeField] private LayerMask _sonarLayerMask = Physics.DefaultRaycastLayers;

        [Header("GPS Normalisation")]
        [SerializeField, Tooltip("World-space position range used to normalise GPS to [0,1].")]
        private float _gpsRange = 10f;

        [Header("IMU")]
        [SerializeField] private float _accelMax = 20f;
        [SerializeField] private float _gyroMax  = 10f;

        [Header("Foot Contacts")]
        [SerializeField, Tooltip("Sphere radius for foot contact CheckSphere.")]
        private float _footContactRadius = 0.015f;
        [SerializeField] private LayerMask _footLayerMask = Physics.DefaultRaycastLayers;

        [Header("Camera")]
        [SerializeField] private int _retinaWidth    = 8;
        [SerializeField] private int _retinaHeight   = 6;
        [SerializeField, Range(0f, 1f)] private float _onThreshold = 0.5f;

        // ------------------------------------------------------------------ //
        // Runtime state
        // ------------------------------------------------------------------ //

        // Main joint array indexed by JointId.
        private ArticulationBody[] _joints;

        // Foot contact check transforms: [foot: 0=L,1=R][corner: 0..3].
        private Transform[,] _footCorners;

        // Camera.
        private Camera        _headCam;
        private RenderTexture _camRT;
        private Texture2D     _readbackTex;

        // IMU.
        private ArticulationBody _torso;
        private Vector3          _prevVelocity;
        private float[]          _prevJointNorm;

        // Bumper flags.
        private float _bumperL, _bumperR;

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
                int pixCount = _retinaWidth * _retinaHeight;
                int total = BaseSensors + 4 * pixCount;
                var n = new string[total];
                int i = 0;
                n[i++] = "nao_s_00_sonar_L";
                n[i++] = "nao_s_01_sonar_R";
                n[i++] = "nao_s_02_accel_x";
                n[i++] = "nao_s_03_accel_y";
                n[i++] = "nao_s_04_accel_z";
                n[i++] = "nao_s_05_gyro_x";
                n[i++] = "nao_s_06_gyro_y";
                n[i++] = "nao_s_07_gps_x";
                n[i++] = "nao_s_08_gps_y";
                n[i++] = "nao_s_09_gps_z";
                n[i++] = "nao_s_10_inertial_roll";
                n[i++] = "nao_s_11_inertial_pitch";
                n[i++] = "nao_s_12_inertial_yaw";
                for (int fp = 0; fp < 8; fp++)
                    n[i++] = $"nao_s_{i:D2}_foot_pressure_{(fp < 4 ? 'L' : 'R')}{fp % 4}";
                n[i++] = "nao_s_21_bumper_L";
                n[i++] = "nao_s_22_bumper_R";
                for (int j = 0; j < NumMotorJoints; j++)
                    n[i++] = $"nao_s_{i:D3}_joint_pos_{j:D2}";
                for (int j = 0; j < NumJointVelocitySensors; j++)
                    n[i++] = $"nao_s_{i:D3}_joint_vel_{j:D2}";
                string[] eyeLabels = { "cam_L_on", "cam_L_off", "cam_R_on", "cam_R_off" };
                for (int q = 0; q < 4; q++)
                    for (int p = 0; p < pixCount; p++)
                        n[i++] = $"nao_s_{i:D3}_{eyeLabels[q]}_{p:D3}";
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
                var n = new string[TotalActuators]
                {
                    "nao_o_00_ShoulderPitchL", "nao_o_01_ShoulderPitchR",
                    "nao_o_02_ShoulderRollL",  "nao_o_03_ShoulderRollR",
                    "nao_o_04_ElbowYawL",      "nao_o_05_ElbowYawR",
                    "nao_o_06_ElbowRollL",     "nao_o_07_ElbowRollR",
                    "nao_o_08_WristYawL",      "nao_o_09_WristYawR",
                    "nao_o_10_HandL",          "nao_o_11_HandR",
                    "nao_o_12_HipYawPitchL",   "nao_o_13_HipYawPitchR",
                    "nao_o_14_HipRollL",       "nao_o_15_HipRollR",
                    "nao_o_16_HipPitchL",      "nao_o_17_HipPitchR",
                    "nao_o_18_KneePitchL",     "nao_o_19_KneePitchR",
                    "nao_o_20_AnklePitchL",    "nao_o_21_AnklePitchR",
                    "nao_o_22_AnkleRollL",     "nao_o_23_AnkleRollR",
                    "nao_o_24_HeadPitch",      "nao_o_25_HeadYaw",
                    "nao_o_26_LED_chest_R",    "nao_o_27_LED_chest_G",
                    "nao_o_28_LED_chest_B",    "nao_o_29_LED_foot_L",
                    "nao_o_30_LED_foot_R",     "nao_o_31_LED_head",
                    "nao_o_32_reserved",       "nao_o_33_reserved",
                    "nao_o_34_reserved",       "nao_o_35_reserved",
                    "nao_o_36_reserved",       "nao_o_37_reserved",
                    "nao_o_38_reserved",       "nao_o_39_reserved",
                };
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
            if (_camRT      != null) { _camRT.Release(); UnityEngine.Object.Destroy(_camRT); }
            if (_readbackTex != null) UnityEngine.Object.Destroy(_readbackTex);
        }

        private void OnCollisionEnter(Collision col) => HandleBumper(col);
        private void OnCollisionStay(Collision col)  => HandleBumper(col);

        private void HandleBumper(Collision col)
        {
            // Simple chest-level bumper: check if contact point is near chest front.
            foreach (ContactPoint cp in col.contacts)
            {
                Vector3 local = transform.InverseTransformPoint(cp.point);
                if (local.z > 0f && Mathf.Abs(local.y) < _torsoHeight * 0.3f)
                {
                    _bumperL = local.x < 0f ? 1f : _bumperL;
                    _bumperR = local.x >= 0f ? 1f : _bumperR;
                }
            }
        }

        // ------------------------------------------------------------------ //
        // Body construction
        // ------------------------------------------------------------------ //

        private void BuildBody()
        {
            _joints      = new ArticulationBody[(int)JointId.Count];
            _footCorners = new Transform[2, 4];

            // Root torso.
            _torso = GetComponent<ArticulationBody>();
            _torso.mass = 5.5f; // NAO V6 ~5.5 kg
            if (GetComponent<BoxCollider>() == null)
            {
                var bc = gameObject.AddComponent<BoxCollider>();
                bc.size   = new Vector3(_torsoWidth, _torsoHeight, _torsoDepth);
                bc.center = Vector3.zero;
            }

            // -- Head --
            var headGo = new GameObject("Head");
            headGo.transform.SetParent(transform);
            headGo.transform.localPosition = new Vector3(0f, _torsoHeight * 0.5f + _headRadius, 0f);
            headGo.transform.localRotation = Quaternion.identity;
            var headCol = headGo.AddComponent<SphereCollider>();
            headCol.radius = _headRadius;
            _joints[(int)JointId.HeadPitch] = AttachRevoluteJoint(headGo, -40f, 30f, 0.6f);
            // HeadYaw is a child of HeadPitch.
            var headYawGo = new GameObject("HeadYaw");
            headYawGo.transform.SetParent(headGo.transform);
            headYawGo.transform.localPosition = Vector3.zero;
            headYawGo.transform.localRotation = Quaternion.identity;
            _joints[(int)JointId.HeadYaw] = AttachRevoluteJoint(headYawGo, -119f, 119f, 0.1f);

            // Head camera.
            SetupHeadCamera(headGo.transform);

            // -- Left Arm --
            BuildArm(isLeft: true);

            // -- Right Arm --
            BuildArm(isLeft: false);

            // -- Left Leg --
            BuildLeg(isLeft: true);

            // -- Right Leg --
            BuildLeg(isLeft: false);

            _prevVelocity = Vector3.zero;
            _bodyBuilt    = true;
        }

        private void BuildArm(bool isLeft)
        {
            float side = isLeft ? -1f : 1f;
            var shoulderGo = new GameObject($"Shoulder{(isLeft ? "L" : "R")}");
            shoulderGo.transform.SetParent(transform);
            shoulderGo.transform.localPosition = new Vector3(side * _torsoWidth * 0.5f,
                                                              _torsoHeight * 0.35f, 0f);

            int pIdx = isLeft ? (int)JointId.ShoulderPitchL : (int)JointId.ShoulderPitchR;
            int rIdx = isLeft ? (int)JointId.ShoulderRollL  : (int)JointId.ShoulderRollR;
            int eyIdx = isLeft ? (int)JointId.ElbowYawL     : (int)JointId.ElbowYawR;
            int erIdx = isLeft ? (int)JointId.ElbowRollL    : (int)JointId.ElbowRollR;
            int wyIdx = isLeft ? (int)JointId.WristYawL     : (int)JointId.WristYawR;
            int hIdx  = isLeft ? (int)JointId.HandL         : (int)JointId.HandR;

            _joints[pIdx] = AttachRevoluteJoint(shoulderGo, -119f, 119f, 0.07f);
            var upperArm  = CreateLimb($"UpperArm{(isLeft ? "L" : "R")}", shoulderGo.transform,
                                       new Vector3(0f, -_upperArmLen * 0.5f, 0f), _upperArmLen);
            _joints[rIdx] = AttachRevoluteJoint(upperArm, isLeft ? -76f : -18f,
                                                           isLeft ? 18f  : 76f, 0.015f);
            var lowerArm  = CreateLimb($"LowerArm{(isLeft ? "L" : "R")}", upperArm.transform,
                                       new Vector3(0f, -_lowerArmLen * 0.5f, 0f), _lowerArmLen);
            _joints[eyIdx] = AttachRevoluteJoint(lowerArm, -119f, 119f, 0.015f);
            _joints[erIdx] = AttachRevoluteJoint(lowerArm, isLeft ? -88.5f : -2f,
                                                            isLeft ? -2f : 88.5f, 0.01f);
            _joints[wyIdx] = AttachRevoluteJoint(lowerArm, -105f, 105f, 0.008f);
            _joints[hIdx]  = AttachRevoluteJoint(lowerArm, 0f, 57.2f, 0.005f);
        }

        private void BuildLeg(bool isLeft)
        {
            float side = isLeft ? -1f : 1f;
            var hipGo  = new GameObject($"Hip{(isLeft ? "L" : "R")}");
            hipGo.transform.SetParent(transform);
            hipGo.transform.localPosition = new Vector3(side * _torsoWidth * 0.3f,
                                                        -_torsoHeight * 0.5f, 0f);

            int ypIdx = isLeft ? (int)JointId.HipYawPitchL : (int)JointId.HipYawPitchR;
            int hrIdx = isLeft ? (int)JointId.HipRollL     : (int)JointId.HipRollR;
            int hpIdx = isLeft ? (int)JointId.HipPitchL    : (int)JointId.HipPitchR;
            int kIdx  = isLeft ? (int)JointId.KneePitchL   : (int)JointId.KneePitchR;
            int apIdx = isLeft ? (int)JointId.AnklePitchL  : (int)JointId.AnklePitchR;
            int arIdx = isLeft ? (int)JointId.AnkleRollL   : (int)JointId.AnkleRollR;

            _joints[ypIdx] = AttachRevoluteJoint(hipGo, -65.7f, 42.4f, 0.06f);
            _joints[hrIdx] = AttachRevoluteJoint(hipGo, isLeft ? -21.7f : -45.3f,
                                                         isLeft ? 45.3f :  21.7f, 0.05f);
            _joints[hpIdx] = AttachRevoluteJoint(hipGo, -88f, 27.7f, 0.08f);

            var thigh = CreateLimb($"Thigh{(isLeft ? "L" : "R")}", hipGo.transform,
                                   new Vector3(0f, -_thighLen * 0.5f, 0f), _thighLen);
            _joints[kIdx]  = AttachRevoluteJoint(thigh, -5.7f, 121.1f, 0.06f);

            var shin = CreateLimb($"Shin{(isLeft ? "L" : "R")}", thigh.transform,
                                  new Vector3(0f, -_shinLen * 0.5f, 0f), _shinLen);
            _joints[apIdx] = AttachRevoluteJoint(shin, -68.1f, 53.4f, 0.04f);
            _joints[arIdx] = AttachRevoluteJoint(shin, isLeft ? -22.8f : -44.1f,
                                                        isLeft ? 44.1f :  22.8f, 0.03f);

            // Foot.
            var footGo = new GameObject($"Foot{(isLeft ? "L" : "R")}");
            footGo.transform.SetParent(shin.transform);
            footGo.transform.localPosition = new Vector3(0f, -_shinLen, 0f);
            footGo.transform.localRotation = Quaternion.identity;
            var footCol = footGo.AddComponent<BoxCollider>();
            footCol.size   = new Vector3(_footWidth, 0.012f, _footLen);
            footCol.center = new Vector3(0f, -0.006f, _footLen * 0.1f);
            var footAb = footGo.AddComponent<ArticulationBody>();
            footAb.mass    = 0.25f;
            footAb.jointType = ArticulationJointType.FixedJoint;

            // Create foot corner contact probes.
            int fi = isLeft ? 0 : 1;
            float[] cx = { -_footWidth * 0.4f, _footWidth * 0.4f,
                           -_footWidth * 0.4f, _footWidth * 0.4f };
            float[] cz = { _footLen * 0.45f, _footLen * 0.45f,
                          -_footLen * 0.35f, -_footLen * 0.35f };
            for (int c = 0; c < 4; c++)
            {
                var corner = new GameObject($"FootCorner{c}");
                corner.transform.SetParent(footGo.transform);
                corner.transform.localPosition = new Vector3(cx[c], -0.01f, cz[c]);
                _footCorners[fi, c] = corner.transform;
            }
        }

        private ArticulationBody AttachRevoluteJoint(GameObject go,
                                                      float lower, float upper, float mass)
        {
            var ab = go.GetComponent<ArticulationBody>() ?? go.AddComponent<ArticulationBody>();
            ab.mass      = mass;
            ab.jointType = ArticulationJointType.RevoluteJoint;
            ab.xDrive    = new ArticulationDrive
            {
                stiffness  = _driveStiffness,
                damping    = _driveDamping,
                forceLimit = float.MaxValue,
                lowerLimit = lower,
                upperLimit = upper,
                target     = 0f
            };
            ab.linearLockX = ArticulationDofLock.LockedMotion;
            ab.linearLockY = ArticulationDofLock.LockedMotion;
            ab.linearLockZ = ArticulationDofLock.LockedMotion;
            return ab;
        }

        private GameObject CreateLimb(string name, Transform parent, Vector3 localPos, float len)
        {
            var go = new GameObject(name);
            go.transform.SetParent(parent);
            go.transform.localPosition = localPos;
            go.transform.localRotation = Quaternion.identity;
            var col = go.AddComponent<CapsuleCollider>();
            col.radius    = _limbRadius;
            col.height    = len;
            col.direction = 1;
            return go;
        }

        private void SetupHeadCamera(Transform headTransform)
        {
            var go = new GameObject("HeadCamera");
            go.transform.SetParent(headTransform);
            go.transform.localPosition = new Vector3(0f, 0f, _headRadius);
            go.transform.localRotation = Quaternion.identity;

            _camRT = new RenderTexture(_retinaWidth, _retinaHeight, 16,
                                       RenderTextureFormat.ARGB32);
            _camRT.filterMode = FilterMode.Point;
            _camRT.Create();

            _headCam                  = go.AddComponent<Camera>();
            _headCam.targetTexture    = _camRT;
            _headCam.fieldOfView      = 60f;
            _headCam.nearClipPlane    = 0.03f;
            _headCam.farClipPlane     = 5f;

            _readbackTex = new Texture2D(_retinaWidth, _retinaHeight,
                                         TextureFormat.RGB24, false);
        }

        // ------------------------------------------------------------------ //
        // Sensor collection
        // ------------------------------------------------------------------ //

        /// <inheritdoc/>
        protected override float[] CollectSensors()
        {
            if (_joints == null) return Array.Empty<float>();

            int pixCount = _retinaWidth * _retinaHeight;
            var sensors  = new float[BaseSensors + 4 * pixCount];

            // --- Sonar [00-01].
            Vector3 chestPos = transform.position;
            sensors[0] = SonarReading(chestPos + transform.TransformDirection(-0.03f, 0f, 0.04f),
                                       transform.forward);
            sensors[1] = SonarReading(chestPos + transform.TransformDirection( 0.03f, 0f, 0.04f),
                                       transform.forward);

            // --- Accelerometer [02-04].
            float dt   = Time.fixedDeltaTime > 0f ? Time.fixedDeltaTime : 0.02f;
            Vector3 vel   = _torso.velocity;
            Vector3 accel = (vel - _prevVelocity) / dt;
            _prevVelocity = vel;
            sensors[2] = Mathf.Clamp01((accel.x + _accelMax) / (2f * _accelMax));
            sensors[3] = Mathf.Clamp01((accel.y + _accelMax) / (2f * _accelMax));
            sensors[4] = Mathf.Clamp01((accel.z + _accelMax) / (2f * _accelMax));

            // --- Gyro [05-06].
            Vector3 angVel = _torso.angularVelocity;
            sensors[5] = Mathf.Clamp01((angVel.x + _gyroMax) / (2f * _gyroMax));
            sensors[6] = Mathf.Clamp01((angVel.y + _gyroMax) / (2f * _gyroMax));

            // --- GPS [07-09]: world position normalised.
            Vector3 pos = transform.position;
            sensors[7] = Mathf.Clamp01((pos.x + _gpsRange) / (2f * _gpsRange));
            sensors[8] = Mathf.Clamp01((pos.y + _gpsRange) / (2f * _gpsRange));
            sensors[9] = Mathf.Clamp01((pos.z + _gpsRange) / (2f * _gpsRange));

            // --- Inertial RPY [10-12]: eulerAngles normalised to [0,1].
            Vector3 euler = transform.eulerAngles;
            sensors[10] = euler.z / 360f; // roll
            sensors[11] = euler.x / 360f; // pitch
            sensors[12] = euler.y / 360f; // yaw

            // --- Foot pressure [13-20]: 4 points per foot.
            for (int foot = 0; foot < 2; foot++)
                for (int c = 0; c < 4; c++)
                {
                    Transform corner = _footCorners[foot, c];
                    bool contact = corner != null &&
                                   Physics.CheckSphere(corner.position, _footContactRadius,
                                                       _footLayerMask,
                                                       QueryTriggerInteraction.Ignore);
                    sensors[13 + foot * 4 + c] = contact ? 1f : 0f;
                }

            // --- Bumpers [21-22].
            sensors[21] = _bumperL;
            sensors[22] = _bumperR;
            _bumperL    = Mathf.Max(0f, _bumperL - Time.fixedDeltaTime * 5f);
            _bumperR    = Mathf.Max(0f, _bumperR - Time.fixedDeltaTime * 5f);

            // --- Joint positions [23..48].
            if (_prevJointNorm == null || _prevJointNorm.Length != NumMotorJoints)
                _prevJointNorm = new float[NumMotorJoints];
            int jointBase = 23;
            for (int j = 0; j < NumMotorJoints; j++)
            {
                float norm = ReadArticulationNorm(_joints[j], 0);
                sensors[jointBase + j] = norm;
            }

            // --- Joint velocities [49..57].
            int velBase = jointBase + NumMotorJoints;
            for (int j = 0; j < NumJointVelocitySensors; j++)
            {
                float curr = sensors[jointBase + j];
                float velNorm = Mathf.Abs(curr - _prevJointNorm[j]) / dt;
                sensors[velBase + j] = Mathf.Clamp01(velNorm / 2f);
            }
            for (int j = 0; j < NumMotorJoints; j++)
                _prevJointNorm[j] = sensors[jointBase + j];

            // --- Camera event channels [58 .. 58 + 4*pixCount - 1].
            FillCameraEvents(sensors, BaseSensors, pixCount);

            return sensors;
        }

        private float SonarReading(Vector3 origin, Vector3 dir)
        {
            float d = _sonarMaxDist;
            if (Physics.SphereCast(origin, 0.05f, dir.normalized, out RaycastHit hit,
                                   _sonarMaxDist, _sonarLayerMask,
                                   QueryTriggerInteraction.Ignore))
                d = hit.distance;
            return 1f - Mathf.Clamp01(d / _sonarMaxDist);
        }

        private void FillCameraEvents(float[] sensors, int startIdx, int pixCount)
        {
            if (_headCam == null || _camRT == null || pixCount == 0) return;
            _headCam.Render();
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
                int row = p / _retinaWidth;
                int col = p % _retinaWidth;
                int mirrored = row * _retinaWidth + (_retinaWidth - 1 - col);
                float rightBright = mirrored >= 0 && mirrored < pixels.Length
                    ? (pixels[mirrored].r / 255f * 0.2126f +
                       pixels[mirrored].g / 255f * 0.7152f +
                       pixels[mirrored].b / 255f * 0.0722f)
                    : bright;

                int lOn = startIdx + p;
                int lOff = startIdx + pixCount + p;
                int rOn = startIdx + 2 * pixCount + p;
                int rOff = startIdx + 3 * pixCount + p;

                if (lOn < sensors.Length) sensors[lOn] = bright > _onThreshold ? bright : 0f;
                if (lOff < sensors.Length) sensors[lOff] = bright <= _onThreshold ? 1f - bright : 0f;
                if (rOn < sensors.Length) sensors[rOn] = rightBright > _onThreshold ? rightBright : 0f;
                if (rOff < sensors.Length) sensors[rOff] = rightBright <= _onThreshold ? 1f - rightBright : 0f;
            }
        }

        // ------------------------------------------------------------------ //
        // Actuator application
        // ------------------------------------------------------------------ //

        /// <inheritdoc/>
        protected override void ApplyActuators(float[] outputs)
        {
            if (_joints == null || outputs == null) return;

            // Channels 0..25: drive motor joints.
            for (int j = 0; j < NumMotorJoints && j < outputs.Length; j++)
            {
                if (_joints[j] == null) continue;
                DriveArticulationNorm(_joints[j], outputs[j], 0);
            }

            // Channels 26..31: LED channels — no physics, values logged only.
            // Channels 32..39: reserved.
        }
    }
}
