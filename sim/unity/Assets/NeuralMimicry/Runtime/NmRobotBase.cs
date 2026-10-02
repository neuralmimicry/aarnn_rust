// NmRobotBase.cs — Abstract MonoBehaviour that drives a robot with an AARNN brain.
// Compatible with Unity 2022.3 LTS+.
//
// Subclass this for each robot phenotype and implement:
//   CollectSensors()   — read joints / sensors → normalised [0,1] floats
//   ApplyActuators()   — write normalised [0,1] outputs → ArticulationBody targets
//   SensorNames        — human-readable channel labels
//   ActuatorNames      — human-readable channel labels

using System;
using System.Threading.Tasks;
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
        public NmBrainConnector brainConnector;

        /// <summary>
        /// When <c>true</c> the component will attempt to reconnect on the next
        /// <c>FixedUpdate</c> after a connection drop.
        /// </summary>
        [Tooltip("Automatically reconnect when the TCP link drops.")]
        public bool autoReconnect = true;
        public NmHabitat Habitat { get; set; }

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

        private Task<StepResult> _pendingStep;
        private float[] _neutralOutputs;
        private bool _connected;
        private bool _attemptedConnection;
        private float _nextRetryTime;
        private bool _handshakeSent;

        private struct StepResult
        {
            public bool ok;
            public float[] outputs;
            public string error;
        }

        /// <summary>Running simulation clock in milliseconds.</summary>
        public float SimulationTimeMs { get; private set; }

        /// <summary>Returns <c>true</c> when the TCP connection is active.</summary>
        public bool IsConnected => _connected;

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
            var appearance = GetComponent<NmRobotAppearance>() ?? gameObject.AddComponent<NmRobotAppearance>();
            appearance.Build(this);
            if (brainConnector == null)
            {
                Debug.LogError($"[NmRobotBase] {name}: brainConnector is not assigned.", this);
                enabled = false;
                return;
            }

            client = brainConnector.CreateClient();
            _neutralOutputs = new float[ActuatorNames.Length];
        }

        /// <summary>
        /// Collects sensors, steps the brain, and applies actuator outputs each
        /// physics frame.
        /// </summary>
        protected virtual void FixedUpdate()
        {
            if (client == null) return;

            SimulationTimeMs += Time.fixedDeltaTime * 1000f * timeScale;

            bool freshOutput = false;
            if (_pendingStep != null)
            {
                if (_pendingStep.IsCompleted)
                {
                    StepResult completed;
                    try { completed = _pendingStep.GetAwaiter().GetResult(); }
                    catch (Exception ex) { completed = new StepResult { error = ex.Message }; }
                    _pendingStep = null;
                    _connected = completed.ok;
                    if (completed.ok)
                    {
                        StepCount++;
                        ApplyActuators(completed.outputs);
                        freshOutput = true;
                    }
                    else
                    {
                        _nextRetryTime = Time.realtimeSinceStartup + 1f;
                        if (!string.IsNullOrEmpty(completed.error))
                            Debug.LogWarning($"[NmRobotBase] {name}: brain step failed — {completed.error}", this);
                    }
                }
            }
            // Apply each sparse frame once; on other ticks drive muscles toward
            // rest. Physics still advances while a network reply is pending.
            if (!freshOutput) ApplyActuators(_neutralOutputs);
            if (_pendingStep != null) return;
            if ((!autoReconnect && !_connected && _attemptedConnection) ||
                Time.realtimeSinceStartup < _nextRetryTime) return;

            // One bounded exchange per robot. A slow brain cannot block Unity's
            // physics/render thread or create an unbounded queue of old samples.
            var sensors = CollectSensors();
            if (sensors == null || sensors.Length != SensorNames.Length) return;
            var timeMs = SimulationTimeMs;
            var sensorNames = SensorNames;
            var actuatorNames = ActuatorNames;
            var transport = client;
            _attemptedConnection = true;
            _pendingStep = Task.Run(() => Exchange(transport, timeMs, sensors, sensorNames, actuatorNames));
        }

        private StepResult Exchange(NmAerClient transport, float timeMs, float[] sensors,
                                    string[] sensorNames, string[] actuatorNames)
        {
            try
            {
                if (!transport.IsConnected || !_handshakeSent)
                {
                    transport.Connect();
                    transport.SendHandshake(sensorNames, actuatorNames);
                    _handshakeSent = true;
                }
                var outputs = new float[actuatorNames.Length];
                if (transport.Step(timeMs, sensors, outputs))
                    return new StepResult { ok = true, outputs = outputs };
                _handshakeSent = false;
                return new StepResult { error = "connection lost" };
            }
            catch (Exception ex)
            {
                _handshakeSent = false;
                return new StepResult { error = ex.Message };
            }
        }

        /// <summary>
        /// Disposes the TCP client when the component is destroyed.
        /// </summary>
        protected virtual void OnDestroy()
        {
            // Dispose after an in-flight exchange, off the physics thread. The
            // client socket has a bounded receive timeout and owns its lock.
            var pending = _pendingStep;
            var closingClient = client;
            if (closingClient != null)
                _ = Task.Run(() =>
                {
                    try { pending?.Wait(); }
                    catch (AggregateException) { /* Disposal still closes the socket. */ }
                    finally { closingClient.Dispose(); }
                });
            client = null;
        }

        /// <summary>
        /// A disabled component stops sampling; an in-flight bounded exchange
        /// may finish and is consumed after re-enable or disposed on destruction.
        /// </summary>
        protected virtual void OnDisable()
        {
            _connected = false;
        }

        // ------------------------------------------------------------------ //
        // Helpers
        // ------------------------------------------------------------------ //

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

        /// <summary>Map an unipolar spike activation to a neutral-centred joint.
        /// A silent output must not command the negative mechanical limit.</summary>
        protected static void DriveArticulationActivation(ArticulationBody body,
                                                          float activation, int axis = 0)
        {
            DriveArticulationNorm(body, 0.5f + 0.5f * Mathf.Clamp01(activation), axis);
        }
    }
}
