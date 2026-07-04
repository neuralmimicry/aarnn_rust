// NmRobotBase.cs — Abstract MonoBehaviour that drives a robot with an AARNN brain.
// Compatible with Unity 2022.3 LTS+.
//
// Subclass this for each robot phenotype and implement:
//   CollectSensors()   — read joints / sensors → normalised [0,1] floats
//   ApplyActuators()   — write normalised [0,1] outputs → ArticulationBody targets
//   SensorNames        — human-readable channel labels
//   ActuatorNames      — human-readable channel labels

using System;
using UnityEngine;

namespace NeuralMimicry
{
    /// <summary>
    /// Base class for all NeuralMimicry-controlled robots.
    /// Owns the lifecycle of one <see cref="NmAerClient"/> and drives it every
    /// <c>FixedUpdate</c> using sensor data collected from the physics scene.
    /// </summary>
    [DisallowMultipleComponent]
    public abstract class NmRobotBase : MonoBehaviour
    {
        // ------------------------------------------------------------------ //
        // Inspector
        // ------------------------------------------------------------------ //

        /// <summary>
        /// Brain configuration asset.  Drag a <see cref="NmBrainConnector"/>
        /// ScriptableObject here in the inspector.
        /// </summary>
        [SerializeField]
        [Tooltip("Brain configuration asset (host, port, encoding).")]
        protected NmBrainConnector brainConnector;

        /// <summary>
        /// When <c>true</c> the component will attempt to reconnect on the next
        /// <c>FixedUpdate</c> after a connection drop.
        /// </summary>
        [Tooltip("Automatically reconnect when the TCP link drops.")]
        public bool autoReconnect = true;

        /// <summary>
        /// Multiplier applied to the physics <c>Time.fixedDeltaTime</c> when
        /// advancing <see cref="SimulationTimeMs"/>.  Set to 1 for real-time.
        /// </summary>
        [Tooltip("Simulation time speed relative to physics fixed-step.")]
        [Range(0.01f, 100f)]
        public float timeScale = 1f;

        // ------------------------------------------------------------------ //
        // Runtime state
        // ------------------------------------------------------------------ //

        /// <summary>The AARNN TCP client created from <see cref="brainConnector"/>.</summary>
        protected NmAerClient client;

        private float[] _sensorBuffer;
        private float[] _outputBuffer;

        private bool _handshakeSent;

        /// <summary>Running simulation clock in milliseconds.</summary>
        public float SimulationTimeMs { get; private set; }

        /// <summary>Returns <c>true</c> when the TCP connection is active.</summary>
        public bool IsConnected => client?.IsConnected ?? false;

        /// <summary>Total number of successful brain steps this session.</summary>
        public int StepCount { get; private set; }

        // ------------------------------------------------------------------ //
        // Abstract interface — subclasses must implement
        // ------------------------------------------------------------------ //

        /// <summary>
        /// Returns the ordered list of sensor channel names.
        /// Length must equal the array returned by <see cref="CollectSensors"/>.
        /// </summary>
        public abstract string[] SensorNames { get; }

        /// <summary>
        /// Returns the ordered list of actuator channel names.
        /// Length must equal the buffer passed to <see cref="ApplyActuators"/>.
        /// </summary>
        public abstract string[] ActuatorNames { get; }

        /// <summary>
        /// Reads the current physics state and returns a normalised [0,1] float
        /// array, one element per sensor channel.  Called once per
        /// <c>FixedUpdate</c> just before sending to the brain.
        /// </summary>
        protected abstract float[] CollectSensors();

        /// <summary>
        /// Applies normalised [0,1] motor outputs to the robot's joints.
        /// Typically drives <see cref="ArticulationBody.SetDriveTarget"/> calls.
        /// Called once per <c>FixedUpdate</c> after a successful brain reply.
        /// </summary>
        /// <param name="outputs">
        /// Motor activation values in [0,1], one per actuator channel.
        /// </param>
        protected abstract void ApplyActuators(float[] outputs);

        // ------------------------------------------------------------------ //
        // MonoBehaviour lifecycle
        // ------------------------------------------------------------------ //

        /// <summary>
        /// Creates the <see cref="NmAerClient"/>, connects to the AARNN server,
        /// and sends the initial JSON handshake.
        /// </summary>
        protected virtual void Start()
        {
            if (brainConnector == null)
            {
                Debug.LogError($"[NmRobotBase] {name}: brainConnector is not assigned.", this);
                enabled = false;
                return;
            }

            client = brainConnector.CreateClient();
            _outputBuffer = new float[ActuatorNames.Length];

            TryConnect();
        }

        /// <summary>
        /// Collects sensors, steps the brain, and applies actuator outputs each
        /// physics frame.
        /// </summary>
        protected virtual void FixedUpdate()
        {
            if (client == null) return;

            // Attempt reconnect if disconnected and autoReconnect is enabled.
            if (!IsConnected)
            {
                if (autoReconnect)
                    TryConnect();
                return;
            }

            // Advance simulation clock.
            SimulationTimeMs += Time.fixedDeltaTime * 1000f * timeScale;

            // Gather sensor data.
            _sensorBuffer = CollectSensors();

            if (_sensorBuffer == null || _sensorBuffer.Length == 0)
                return;

            // Ensure output buffer is sized correctly.
            if (_outputBuffer == null || _outputBuffer.Length != ActuatorNames.Length)
                _outputBuffer = new float[ActuatorNames.Length];

            // Send → receive.
            bool ok = client.Step(SimulationTimeMs, _sensorBuffer, _outputBuffer);
            if (ok)
            {
                StepCount++;
                ApplyActuators(_outputBuffer);
            }
        }

        /// <summary>
        /// Disposes the TCP client when the component is destroyed.
        /// </summary>
        protected virtual void OnDestroy()
        {
            client?.Dispose();
            client = null;
        }

        /// <summary>
        /// Called when the component is disabled — drops the TCP connection but
        /// does not destroy the client so it can reconnect on re-enable.
        /// </summary>
        protected virtual void OnDisable()
        {
            // Let the client attempt a clean close; reconnect will happen in
            // FixedUpdate when re-enabled and autoReconnect is true.
            _handshakeSent = false;
        }

        // ------------------------------------------------------------------ //
        // Helpers
        // ------------------------------------------------------------------ //

        private void TryConnect()
        {
            if (client == null) return;
            try
            {
                client.Connect();
                if (!_handshakeSent)
                {
                    client.SendHandshake(SensorNames, ActuatorNames);
                    _handshakeSent = true;
                }
                Debug.Log($"[NmRobotBase] {name}: connected to brain '{brainConnector.brainId}'.");
            }
            catch (Exception ex)
            {
                Debug.LogWarning($"[NmRobotBase] {name}: connect failed — {ex.Message}");
            }
        }

        // ------------------------------------------------------------------ //
        // Utility helpers for subclasses
        // ------------------------------------------------------------------ //

        /// <summary>
        /// Convenience: reads the current reduced position of an
        /// <see cref="ArticulationBody"/> drive and normalises it from the drive's
        /// [lowerLimit, upperLimit] to [0, 1].
        /// </summary>
        /// <param name="body">The articulation body to read.</param>
        /// <param name="axis">The drive axis index (0=X, 1=Y, 2=Z).</param>
        protected static float ReadArticulationNorm(ArticulationBody body, int axis = 0)
        {
            if (body == null) return 0f;
            var drive = axis switch
            {
                1 => body.yDrive,
                2 => body.zDrive,
                _ => body.xDrive,
            };
            float range = drive.upperLimit - drive.lowerLimit;
            if (Mathf.Approximately(range, 0f)) return 0f;
            float pos = body.jointPosition[axis];
            return Mathf.Clamp01((pos - drive.lowerLimit) / range);
        }

        /// <summary>
        /// Convenience: drives an <see cref="ArticulationBody"/> to a normalised
        /// target in [0,1] by mapping it to the drive's [lowerLimit, upperLimit].
        /// </summary>
        /// <param name="body">The articulation body to drive.</param>
        /// <param name="normTarget">Normalised target in [0,1].</param>
        /// <param name="axis">The drive axis index (0=X, 1=Y, 2=Z).</param>
        protected static void DriveArticulationNorm(ArticulationBody body,
                                                    float normTarget, int axis = 0)
        {
            if (body == null) return;

            var drive = axis switch
            {
                1 => body.yDrive,
                2 => body.zDrive,
                _ => body.xDrive,
            };

            float target = Mathf.Lerp(drive.lowerLimit, drive.upperLimit,
                                      Mathf.Clamp01(normTarget));
            drive.target = target;

            switch (axis)
            {
                case 1: body.yDrive = drive; break;
                case 2: body.zDrive = drive; break;
                default: body.xDrive = drive; break;
            }
        }
    }
}
