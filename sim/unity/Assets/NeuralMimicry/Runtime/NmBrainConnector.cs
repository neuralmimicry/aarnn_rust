// NmBrainConnector.cs — ScriptableObject that describes one AARNN brain instance.
// Compatible with Unity 2022.3 LTS+.

using System.Collections.Generic;
using UnityEngine;

namespace NeuralMimicry
{
    /// <summary>
    /// A <see cref="ScriptableObject"/> that stores the connection parameters
    /// for one AARNN brain server.  Create instances via the Unity Project window:
    /// Assets / Create / NeuralMimicry / Brain Connector.
    /// </summary>
    [CreateAssetMenu(menuName = "NeuralMimicry/Brain Connector",
                     fileName = "NmBrainConnector",
                     order = 100)]
    public sealed class NmBrainConnector : ScriptableObject
    {
        // ------------------------------------------------------------------ //
        // Identity
        // ------------------------------------------------------------------ //

        /// <summary>
        /// Logical brain identifier.  Must match the robot phenotype the AARNN
        /// server was built for, e.g. <c>"celegans"</c>, <c>"banc"</c>,
        /// <c>"fafb"</c>, <c>"hexapod"</c>, <c>"nao"</c>, <c>"zebrafish"</c>.
        /// This value is used as the key in <see cref="Registered"/>.
        /// </summary>
        [Tooltip("Logical brain ID — must match the robot type on the AARNN server.")]
        public string brainId = "celegans";

        // ------------------------------------------------------------------ //
        // Transport
        // ------------------------------------------------------------------ //

        /// <summary>Hostname or IP address of the AARNN TCP server.</summary>
        [Tooltip("Hostname or IP of the AARNN TCP server.")]
        public string tcpHost = "127.0.0.1";

        /// <summary>TCP port the AARNN server is listening on.</summary>
        [Tooltip("TCP port of the AARNN server.")]
        public int tcpPort = 7890;

        // ------------------------------------------------------------------ //
        // Encoding
        // ------------------------------------------------------------------ //

        /// <summary>
        /// Normalised sensor value [0,1] above which an AER spike is emitted.
        /// Values at or below this threshold are silent.
        /// </summary>
        [Tooltip("Sensor values above this threshold generate an AER spike event.")]
        [Range(0f, 1f)]
        public float spikeThreshold = 0.5f;

        /// <summary>
        /// When <c>true</c> (default) encode via the AER1 binary protocol.
        /// Set to <c>false</c> to use the raw-float wire format for servers that
        /// do not implement AER.
        /// </summary>
        [Tooltip("Use AER1 binary encoding. Disable for raw-float fallback servers.")]
        public bool useAerEncoding = true;

        // ------------------------------------------------------------------ //
        // Static registry
        // ------------------------------------------------------------------ //

        /// <summary>
        /// Global registry of all enabled <see cref="NmBrainConnector"/> assets,
        /// keyed by <see cref="brainId"/>.  Connectors register themselves
        /// automatically on <c>OnEnable</c> and deregister on <c>OnDisable</c>.
        /// </summary>
        public static readonly Dictionary<string, NmBrainConnector> Registered
            = new Dictionary<string, NmBrainConnector>();

        // ------------------------------------------------------------------ //
        // Lifecycle
        // ------------------------------------------------------------------ //

        private void OnEnable() => Register();
        private void OnDisable() => Unregister();

        /// <summary>
        /// Inserts this connector into <see cref="Registered"/> under
        /// <see cref="brainId"/>.  Replaces any previously registered connector
        /// with the same id.
        /// </summary>
        public void Register()
        {
            if (string.IsNullOrEmpty(brainId)) return;
            Registered[brainId] = this;
        }

        /// <summary>
        /// Removes this connector from <see cref="Registered"/>.
        /// Only removes the entry when it still points to this instance.
        /// </summary>
        public void Unregister()
        {
            if (string.IsNullOrEmpty(brainId)) return;
            if (Registered.TryGetValue(brainId, out var current) && current == this)
                Registered.Remove(brainId);
        }

        // ------------------------------------------------------------------ //
        // Factory
        // ------------------------------------------------------------------ //

        /// <summary>
        /// Creates a fully configured <see cref="NmAerClient"/> from this
        /// connector's settings.  The caller is responsible for connecting and
        /// disposing the client.
        /// </summary>
        public NmAerClient CreateClient()
        {
            return new NmAerClient(tcpHost, tcpPort, spikeThreshold, useAerEncoding);
        }

        /// <summary>
        /// Looks up a registered connector by <paramref name="id"/> and returns
        /// it, or <c>null</c> if none is registered under that id.
        /// </summary>
        public static NmBrainConnector Find(string id)
        {
            Registered.TryGetValue(id, out var c);
            return c;
        }
    }
}
