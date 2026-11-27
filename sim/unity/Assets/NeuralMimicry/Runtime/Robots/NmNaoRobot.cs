// SPDX-FileCopyrightText: masterkiga
// SPDX-License-Identifier: MIT
//
// Licensed under the MIT License. See LICENSE in the project root.
// NmNaoRobot.cs — Unity C# MonoBehaviour for the NAO V6 humanoid robot.
// Compatible with Unity 2022.3 LTS+.
//
// Body and drives are the OEM NAO of the MuJoCo reference, frozen from the
// generated record into NmNaoOem.generated.cs (scripts/generate_nao_engine_tables.py):
// 41 ArticulationBody links (torso + 40 hinges incl. 8 phalanges per hand) in
// the OEM zero pose, H25 link offsets, per-link masses and inertia, OEM joint
// ranges, position-servo gains kp = torque / 0.1 rad, back-EMF damping, torque
// and speed limits. Self-collision is off, as in the reference.
//
// Sensor channels (250 at the default 8×6 retina; this body's legacy layout):
//   [00-01]   2 sonar (L, R): NAO V6 chest transducers, 60 deg cone of
//             91 rays, 0.20-0.80 m, specular loss above 22.5 deg incidence
//             (the record's legacy-range contract)
//   [02-04]   3 accelerometer XYZ
//   [05-06]   2 gyro XY
//   [07-09]   3 GPS XYZ (world position, metre-scale, normalised per axis)
//   [10-12]   3 inertial (roll, pitch, yaw) normalised to [0,1]
//   [13-20]   8 foot pressure (L4 + R4 sole corners: toe L/R, heel L/R)
//   [21-22]   2 bumpers (L foot, R foot): the NAO V6 toe switches, never
//             pressed by standing on the floor
//   [23-48]   26 joint positions, (q - lo)/(hi - lo), legacy joint order
//             (see LegacySensorJoints; Hand = Phalanx1)
//   [49-57]    9 joint-velocity channels (first nine of the above)
//   [58 .. 58 + 4*W*H - 1]  head camera event channels (on/off × L/R)
//
// Actuator channels (40): the reference NAO output contract
// (io.output_names of generated/nao.json, NmNaoOem.OutputNames). Each joint
// reads its record readout channel. A graded (pulse-density) brain is decoded
// as Actuation does (actuation.rs): target = lo + density (hi - lo), with the
// record's decode_tau_s (0.2 s) and servo wake-up (from the zero pose over
// 1 s), so a brain that stands the MuJoCo NAO drives the same joints here.

using System;
using System.Collections.Generic;
using UnityEngine;

namespace NeuralMimicry
{
    /// <summary>
    /// NeuralMimicry-controlled simulation of the NAO V6 humanoid robot, built
    /// from the OEM link tree of the MuJoCo reference (NmNaoOem). Sensor
    /// modalities mirror the Webots NAO NN controller: sonar, IMU, GPS, foot
    /// pressure, bumpers, and a downsampled head-camera event stream.
    /// </summary>
    [RequireComponent(typeof(ArticulationBody))]
    public sealed class NmNaoRobot : NmRobotBase
    {
        protected override void Start()
        {
            NmNaoChat.Attach(this);
            base.Start();
        }
        // ------------------------------------------------------------------ //
        // Constants
        // ------------------------------------------------------------------ //

        private const int BaseSensors = 58;
        private const int NumJointPositionSensors = 26;
        private const int NumJointVelocitySensors = 9;

        /// <summary>Joints behind the legacy joint-position channels [23..48].</summary>
        private static readonly string[] LegacySensorJoints =
        {
            "LShoulderPitch", "RShoulderPitch", "LShoulderRoll", "RShoulderRoll",
            "LElbowYaw", "RElbowYaw", "LElbowRoll", "RElbowRoll",
            "LWristYaw", "RWristYaw", "LPhalanx1", "RPhalanx1",
            "LHipYawPitch", "RHipYawPitch", "LHipRoll", "RHipRoll",
            "LHipPitch", "RHipPitch", "LKneePitch", "RKneePitch",
            "LAnklePitch", "RAnklePitch", "LAnkleRoll", "RAnkleRoll",
            "HeadPitch", "HeadYaw",
        };
        private static readonly string[] LegacySensorLabels =
        {
            "ShoulderPitchL", "ShoulderPitchR", "ShoulderRollL", "ShoulderRollR",
            "ElbowYawL", "ElbowYawR", "ElbowRollL", "ElbowRollR",
            "WristYawL", "WristYawR", "HandL", "HandR",
            "HipYawPitchL", "HipYawPitchR", "HipRollL", "HipRollR",
            "HipPitchL", "HipPitchR", "KneePitchL", "KneePitchR",
            "AnklePitchL", "AnklePitchR", "AnkleRollL", "AnkleRollR",
            "HeadPitch", "HeadYaw",
        };

        /// <summary>CameraTop of the reference, relative to the HeadPitch link (MuJoCo frame, m).</summary>
        private static readonly double[] HeadCameraM = { 0.0539, 0.0, 0.0679 };

        // ------------------------------------------------------------------ //
        // Inspector
        // ------------------------------------------------------------------ //

        [Header("Sonar")]
        [SerializeField, Tooltip("Max sonar range in metres.")]
        private float _sonarMaxDist = 0.80f;   // NAO V6: 0.20-0.80 m
        private const float SonarMinDist = 0.20f;
        private const float SonarIncidenceMaxDeg = 22.5f;
        // MuJoCo torso frame (x fwd, y left, z up), Webots V5 Nao.proto.
        private static readonly double[] SonarPosLeft = { 0.0477, 0.0416, 0.0509 };
        private static readonly double[] SonarAxisLeft = { 0.8754, 0.4226, -0.2346 };
        private const double SonarApertureRad = 1.04;
        [SerializeField] private LayerMask _sonarLayerMask = Physics.DefaultRaycastLayers;

        [Header("GPS Normalisation")]
        [SerializeField, Tooltip("World-space position range used to normalise GPS to [0,1].")]
        private float _gpsRange = 10f;

        [Header("IMU")]
        [SerializeField] private float _accelMax = 20f;
        [SerializeField] private float _gyroMax  = 10f;

        [Header("Foot Contacts")]
        [SerializeField, Tooltip("Sphere radius for foot contact probes below each sole corner.")]
        private float _footContactRadius = 0.004f;
        [SerializeField] private LayerMask _footLayerMask = Physics.DefaultRaycastLayers;

        [Header("Camera")]
        [SerializeField] private int _retinaWidth    = 8;
        [SerializeField] private int _retinaHeight   = 6;
        [SerializeField, Range(0f, 1f)] private float _onThreshold = 0.5f;

        // ------------------------------------------------------------------ //
        // Runtime state
        // ------------------------------------------------------------------ //

        // One ArticulationBody per OEM hinge, NmNaoOem.Links order.
        private ArticulationBody[] _links;
        private int[] _sensorLinks;
        // Graded (pulse-density) brain: the record's readout channels,
        // decode_tau_s and wake (the reference Actuation decode).
        private readonly NmGradedJoints _graded = BuildDecoder();

        // Foot contact probes: [foot: 0=L,1=R][corner: 0..3].
        private Transform[,] _footCorners;
        private readonly HashSet<Collider> _ownColliders = new HashSet<Collider>();
        private readonly Collider[] _overlap = new Collider[16];

        // Camera.
        private Camera        _headCam;
        private RenderTexture _camRT;
        private Texture2D     _readbackTex;

        // IMU.
        private ArticulationBody _torso;
        private Vector3          _prevVelocity;
        private float[]          _prevJointNorm;

        // Bumper flags (toe switches) and their probes [foot, left/right].
        private float _bumperL, _bumperR;
        private Transform[,] _toeBumpers;
        private const float ToeBumperRadius = 0.012f;
        private static Vector3[] _sonarConeLeft;

        private bool _bodyBuilt;

        private string[] _sensorNamesCache;

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
                for (int j = 0; j < NumJointPositionSensors; j++)
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
        public override string[] ActuatorNames => _actuatorNames;
        private readonly string[] _actuatorNames = (string[])NmNaoOem.OutputNames.Clone();

        /// <summary>Legacy label of joint-position channel <paramref name="index"/> (0..25).</summary>
        public static string JointPositionLabel(int index) => LegacySensorLabels[index];

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

        /// The record's sonar cone (scripts/embodiment_neural.py cone_rays):
        /// the axis plus 5 rings to half the aperture, ring k with 6k rays,
        /// in Unity torso-local axes (left sonar; the right one mirrors x).
        private static Vector3[] SonarCone()
        {
            if (_sonarConeLeft != null) return _sonarConeLeft;
            var a = U(SonarAxisLeft).normalized;
            var r = Mathf.Abs(a.y) < 0.9f ? Vector3.up : Vector3.right;
            var u = Vector3.Cross(r, a).normalized;
            var w = Vector3.Cross(a, u);
            var rays = new List<Vector3> { a };
            for (int ring = 1; ring <= 5; ring++)
            {
                float t = (float)(SonarApertureRad / 2 * ring / 5);
                int per = 6 * ring;
                for (int j = 0; j < per; j++)
                {
                    float phi = 2f * Mathf.PI * (j + 0.5f * (ring % 2)) / per;
                    rays.Add((Mathf.Cos(t) * a + Mathf.Sin(t) * (Mathf.Cos(phi) * u + Mathf.Sin(phi) * w)).normalized);
                }
            }
            _sonarConeLeft = rays.ToArray();
            return _sonarConeLeft;
        }

        // ------------------------------------------------------------------ //
        // Body construction (OEM link tree, MuJoCo frame -> Unity)
        // ------------------------------------------------------------------ //

        /// <summary>MuJoCo (x fwd, y left, z up) to Unity (x right, y up, z fwd): EmbodimentBasis.Polar.</summary>
        private static Vector3 U(double[] a) => new Vector3((float)-a[1], (float)a[2], (float)a[0]);

        /// <summary>MuJoCo wxyz quaternion to Unity: EmbodimentBasis.Rotation.</summary>
        private static Quaternion Q(double[] q) =>
            new Quaternion((float)q[2], (float)-q[3], (float)-q[1], (float)q[0]);

        /// <summary>MuJoCo half extents (x, y, z) to a Unity box size.</summary>
        private static Vector3 Size(double[] h) =>
            new Vector3((float)(2 * h[1]), (float)(2 * h[2]), (float)(2 * h[0]));

        private static NmGradedJoints BuildDecoder()
        {
            var links = NmNaoOem.Links;
            var readouts = new NmReadout[links.Length][];
            for (int j = 0; j < links.Length; j++)
                readouts[j] = new[] { new NmReadout(links[j].Channel, 1.0) };
            var decoder = new NmGradedJoints(readouts);
            for (int j = 0; j < links.Length; j++)
            {
                var k = links[j];
                if (k.DecodeTauS > 0.0) decoder.SetDecodeTau(j, k.DecodeTauS);
                if (k.Wake) decoder.SetWake(j, k.WakeFrom, k.WakeSeconds, k.Lo, k.Hi);
            }
            return decoder;
        }

        private void BuildBody()
        {
            var links = NmNaoOem.Links;
            _links = new ArticulationBody[links.Length];
            _footCorners = new Transform[2, 4];
            var own = new List<Collider>();

            // Root torso (free articulation root).
            _torso = GetComponent<ArticulationBody>();
            var torsoBox = GetComponent<BoxCollider>();
            if (torsoBox == null) torsoBox = gameObject.AddComponent<BoxCollider>();
            torsoBox.center = U(NmNaoOem.TorsoBoxPositionM);
            torsoBox.size = Size(NmNaoOem.TorsoBoxHalfM);
            own.AddRange(GetComponents<Collider>());
            SetMass(_torso, NmNaoOem.TorsoMassKg, NmNaoOem.TorsoComM, NmNaoOem.TorsoInertiaKgM2,
                    NmNaoOem.TorsoInertiaFrameWxyz);

            var soleGrip = new PhysicMaterial("NAO sole")
            {
                dynamicFriction = 1f, staticFriction = 1f, bounciness = 0f,
                frictionCombine = PhysicMaterialCombine.Maximum,
            };
            for (int i = 0; i < links.Length; i++)
            {
                var k = links[i];
                var parent = k.Parent < 0 ? transform : _links[k.Parent].transform;
                var go = new GameObject(k.Name);
                go.transform.SetParent(parent, false);
                go.transform.localPosition = U(k.PositionM);
                go.transform.localRotation = Quaternion.identity;
                var collider = AddCollider(go, k);
                own.Add(collider);
                var ab = go.AddComponent<ArticulationBody>();
                ab.jointType = ArticulationJointType.RevoluteJoint;
                ab.anchorPosition = Vector3.zero;
                // A MuJoCo hinge's +angle about axis a is a Unity rotation about
                // -Polar(a) (the basis change is a reflection), so the joint
                // reads and drives the reference angle directly.
                ab.anchorRotation = Quaternion.FromToRotation(Vector3.right, -U(k.Axis));
                ab.twistLock = ArticulationDofLock.LimitedMotion;
                ab.linearDamping = 0f;
                ab.angularDamping = 0f;
                ab.jointFriction = 0f;
                ab.maxJointVelocity = (float)k.VelocityRadS;
                // ArticulationDrive: target/limits in degrees, stiffness N m/rad,
                // damping N m s/rad, force limit N m (PhysX reduced coordinates).
                ab.xDrive = new ArticulationDrive
                {
                    stiffness  = (float)k.Kp,
                    damping    = (float)k.Kd,
                    forceLimit = (float)k.EffortNm,
                    lowerLimit = (float)(k.Lo * Mathf.Rad2Deg),
                    upperLimit = (float)(k.Hi * Mathf.Rad2Deg),
                    target     = RestDegrees(k),
                };
                SetMass(ab, k.MassKg, k.ComM, k.InertiaKgM2, k.InertiaFrameWxyz);
                _links[i] = ab;
                if (k.Name == "LAnkleRoll" || k.Name == "RAnkleRoll")
                {
                    collider.sharedMaterial = soleGrip;
                    AddFootCorners(go.transform, k, k.Name[0] == 'L' ? 0 : 1);
                }
                if (k.Name == "HeadPitch")
                    SetupHeadCamera(go.transform);
            }
            // The reference excludes every self-contact pair (graph exclusions).
            foreach (var c in own) _ownColliders.Add(c);
            for (int a = 0; a < own.Count; a++)
                for (int b = a + 1; b < own.Count; b++)
                    Physics.IgnoreCollision(own[a], own[b]);

            _sensorLinks = new int[NumJointPositionSensors];
            for (int s = 0; s < NumJointPositionSensors; s++)
                _sensorLinks[s] = Array.FindIndex(links, k => k.Name == LegacySensorJoints[s]);
            _prevVelocity = Vector3.zero;
            _bodyBuilt    = true;
        }

        private static Collider AddCollider(GameObject link, NmNaoOem.Link k)
        {
            var go = new GameObject(k.Name + "_collision");
            go.transform.SetParent(link.transform, false);
            go.transform.localPosition = U(k.ColliderPositionM);
            go.transform.localRotation = Q(k.ColliderFrameWxyz);
            var h = k.ColliderHalfM;
            switch (k.Collider)
            {
                case NmNaoOem.Shape.Sphere:
                {
                    var s = go.AddComponent<SphereCollider>();
                    s.radius = (float)h[0];
                    return s;
                }
                case NmNaoOem.Shape.Ellipsoid:
                {
                    // Engineering approximation: the head ellipsoid as a sphere.
                    var s = go.AddComponent<SphereCollider>();
                    s.radius = (float)((h[0] + h[1] + h[2]) / 3.0);
                    return s;
                }
                case NmNaoOem.Shape.Capsule:
                case NmNaoOem.Shape.Cylinder:
                {
                    // MuJoCo capsule/cylinder axis is local z = Unity local y.
                    var c = go.AddComponent<CapsuleCollider>();
                    c.direction = 1;
                    c.radius = (float)h[0];
                    double length = k.Collider == NmNaoOem.Shape.Capsule ? 2 * (h[1] + h[0]) : 2 * h[1];
                    c.height = Mathf.Max((float)length, 2f * (float)h[0]);
                    return c;
                }
                default:
                {
                    var b = go.AddComponent<BoxCollider>();
                    b.size = Size(h);
                    return b;
                }
            }
        }

        private static void SetMass(ArticulationBody body, double mass, double[] com, double[] inertia, double[] frame)
        {
            body.mass = (float)mass;
            body.centerOfMass = U(com);
            body.inertiaTensor = new Vector3((float)inertia[1], (float)inertia[2], (float)inertia[0]);
            body.inertiaTensorRotation = Q(frame);
        }

        /// <summary>Rest pose of a joint (its wake pose, else the zero pose) in degrees.</summary>
        private static float RestDegrees(NmNaoOem.Link k)
        {
            double rest = Math.Max(k.Lo, Math.Min(k.Hi, k.Wake ? k.WakeFrom : 0.0));
            return (float)(rest * Mathf.Rad2Deg);
        }

        private void AddFootCorners(Transform ankle, NmNaoOem.Link k, int foot)
        {
            // Sole box corners (toe left, toe right, heel left, heel right),
            // probes just below the sole.
            var c = k.ColliderPositionM;
            var h = k.ColliderHalfM;
            double bottom = c[2] - h[2] - _footContactRadius - 0.001;
            double[][] corners =
            {
                new[] { c[0] + 0.9 * h[0], c[1] + 0.9 * h[1], bottom },
                new[] { c[0] + 0.9 * h[0], c[1] - 0.9 * h[1], bottom },
                new[] { c[0] - 0.9 * h[0], c[1] + 0.9 * h[1], bottom },
                new[] { c[0] - 0.9 * h[0], c[1] - 0.9 * h[1], bottom },
            };
            for (int i = 0; i < 4; i++)
            {
                var corner = new GameObject($"FootCorner{i}");
                corner.transform.SetParent(ankle, false);
                corner.transform.localPosition = U(corners[i]);
                _footCorners[foot, i] = corner.transform;
            }
            // Toe bumper switches (NAO V6: two at each toe, URDF y +0.019 /
            // -0.014 on the left foot, mirrored): probes just ahead of the toe
            // face, 6 mm and more above the sole.
            if (_toeBumpers == null) _toeBumpers = new Transform[2, 2];
            double sole = c[2] - h[2];
            double inner = foot == 0 ? -0.014 : 0.014, outer = foot == 0 ? 0.019 : -0.019;
            double[][] toes =
            {
                new[] { c[0] + h[0] + 0.004, foot == 0 ? outer : inner, sole + 0.018 },
                new[] { c[0] + h[0] + 0.004, foot == 0 ? inner : outer, sole + 0.018 },
            };
            for (int b = 0; b < 2; b++)
            {
                var probe = new GameObject($"ToeBumper{b}");
                probe.transform.SetParent(ankle, false);
                probe.transform.localPosition = U(toes[b]);
                _toeBumpers[foot, b] = probe.transform;
            }
        }

        private void SetupHeadCamera(Transform headTransform)
        {
            var go = new GameObject("HeadCamera");
            go.transform.SetParent(headTransform, false);
            go.transform.localPosition = U(HeadCameraM);
            go.transform.localRotation = Quaternion.identity;

            _camRT = new RenderTexture(_retinaWidth, _retinaHeight, 16,
                                       RenderTextureFormat.ARGB32);
            _camRT.filterMode = FilterMode.Point;
            _camRT.Create();

            _headCam                  = go.AddComponent<Camera>();
            _headCam.targetTexture    = _camRT;
            _headCam.fieldOfView      = 60.7f; // reference CameraTop fovy
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
            if (_links == null) return Array.Empty<float>();

            int pixCount = _retinaWidth * _retinaHeight;
            var sensors  = new float[BaseSensors + 4 * pixCount];

            // --- Sonar [00-01].
            for (int side = 0; side < 2; side++)
            {
                float mirror = side == 0 ? 1f : -1f;   // Unity x = -MuJoCo y
                Vector3 local = U(SonarPosLeft);
                local.x *= mirror;
                sensors[side] = SonarReading(transform.TransformPoint(local), mirror);
            }

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

            // --- Foot pressure [13-20]: 4 sole corners per foot, own body excluded.
            for (int foot = 0; foot < 2; foot++)
                for (int c = 0; c < 4; c++)
                {
                    Transform corner = _footCorners[foot, c];
                    sensors[13 + foot * 4 + c] = corner != null && Touching(corner.position) ? 1f : 0f;
                }

            // --- Bumpers [21-22]: either toe switch of each foot.
            for (int foot = 0; foot < 2; foot++)
            {
                bool pressed = false;
                for (int b = 0; b < 2 && _toeBumpers != null; b++)
                {
                    Transform probe = _toeBumpers[foot, b];
                    pressed |= probe != null && Touching(probe.position, ToeBumperRadius);
                }
                if (foot == 0) _bumperL = pressed ? 1f : 0f; else _bumperR = pressed ? 1f : 0f;
            }
            sensors[21] = _bumperL;
            sensors[22] = _bumperR;

            // --- Joint positions [23..48]: (q - lo)/(hi - lo), q in radians.
            if (_prevJointNorm == null || _prevJointNorm.Length != NumJointPositionSensors)
                _prevJointNorm = new float[NumJointPositionSensors];
            const int jointBase = 23;
            for (int s = 0; s < NumJointPositionSensors; s++)
            {
                int j = _sensorLinks[s];
                var k = NmNaoOem.Links[j];
                double q = _links[j].jointPosition.dofCount > 0 ? _links[j].jointPosition[0] : 0.0;
                sensors[jointBase + s] = Mathf.Clamp01((float)((q - k.Lo) / (k.Hi - k.Lo)));
            }

            // --- Joint velocities [49..57].
            int velBase = jointBase + NumJointPositionSensors;
            for (int j = 0; j < NumJointVelocitySensors; j++)
            {
                float curr = sensors[jointBase + j];
                float velNorm = Mathf.Abs(curr - _prevJointNorm[j]) / dt;
                sensors[velBase + j] = Mathf.Clamp01(velNorm / 2f);
            }
            for (int j = 0; j < NumJointPositionSensors; j++)
                _prevJointNorm[j] = sensors[jointBase + j];

            // --- Camera event channels [58 .. 58 + 4*pixCount - 1].
            FillCameraEvents(sensors, BaseSensors, pixCount);

            return sensors;
        }

        private bool Touching(Vector3 point) => Touching(point, _footContactRadius);

        private bool Touching(Vector3 point, float radius)
        {
            int n = Physics.OverlapSphereNonAlloc(point, radius, _overlap, _footLayerMask,
                                                  QueryTriggerInteraction.Ignore);
            for (int i = 0; i < n; i++)
                if (!_ownColliders.Contains(_overlap[i]))
                    return true;
            return false;
        }

        /// Nearest echo over the cone; a ray striking a surface more than
        /// 22.5 deg from its normal is reflected away (no echo).
        private float SonarReading(Vector3 origin, float mirror)
        {
            float d = _sonarMaxDist;
            foreach (Vector3 ray in SonarCone())
            {
                Vector3 dir = transform.TransformDirection(new Vector3(ray.x * mirror, ray.y, ray.z));
                foreach (var hit in Physics.RaycastAll(origin, dir, _sonarMaxDist, _sonarLayerMask,
                                                       QueryTriggerInteraction.Ignore))
                {
                    if (_ownColliders.Contains(hit.collider) || hit.distance >= d) continue;
                    if (Vector3.Angle(-dir, hit.normal) > SonarIncidenceMaxDeg) continue;
                    d = hit.distance;
                }
            }
            d = Mathf.Max(d, SonarMinDist);
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

        private static void SetTargetDegrees(ArticulationBody body, float degrees)
        {
            var drive = body.xDrive;
            drive.target = degrees;
            body.xDrive = drive;
        }

        /// <inheritdoc/>
        protected override void ApplyActuators(float[] outputs)
        {
            if (_links == null || outputs == null) return;
            var links = NmNaoOem.Links;

            if (IsGraded)
            {
                // Reference decode (actuation.rs): record readout channel,
                // decode_tau_s and wake; target in [lo, hi] rad.
                if (FreshFrame)
                {
                    _graded.Accept(outputs);
                    _graded.Advance();
                }
                for (int j = 0; j < links.Length; j++)
                    SetTargetDegrees(_links[j], (float)(_graded.Target(j, links[j].Lo, links[j].Hi) * Mathf.Rad2Deg));
                return;
            }

            for (int j = 0; j < links.Length; j++)
            {
                var k = links[j];
                // A spike frame maps activation 0..1 onto the upper half of the
                // range (silence must not command the lower limit); a tick
                // without a reply holds the rest pose.
                float degrees = FreshFrame && k.Channel < outputs.Length
                    ? Mathf.Lerp((float)(k.Lo * Mathf.Rad2Deg), (float)(k.Hi * Mathf.Rad2Deg),
                                 0.5f + 0.5f * Mathf.Clamp01(outputs[k.Channel]))
                    : RestDegrees(k);
                SetTargetDegrees(_links[j], degrees);
            }
        }

        /// <inheritdoc/>
        protected override void ResetGradedDecode() => _graded.Reset();
    }
}
