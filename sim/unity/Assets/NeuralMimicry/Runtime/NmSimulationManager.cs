// NmSimulationManager.cs — Scene singleton that tracks all AARNN robots.
// Compatible with Unity 2022.3 LTS+.

using System.Collections.Generic;
using UnityEngine;

namespace NeuralMimicry
{
    /// <summary>
    /// Scene-level singleton that owns the list of active <see cref="NmRobotBase"/>
    /// robots and provides an optional on-screen debug overlay.
    /// </summary>
    /// <remarks>
    /// Place one instance in the scene.  If <see cref="autoDiscoverRobots"/> is
    /// enabled the manager will populate <see cref="robots"/> automatically on
    /// Start by searching the scene.  Otherwise populate the list manually (e.g.
    /// via the inspector or through scripted prefab spawning).
    /// </remarks>
    [DisallowMultipleComponent]
    public sealed class NmSimulationManager : MonoBehaviour
    {
        // ------------------------------------------------------------------ //
        // Singleton
        // ------------------------------------------------------------------ //

        /// <summary>The active <see cref="NmSimulationManager"/> in the scene.</summary>
        public static NmSimulationManager Instance { get; private set; }

        // ------------------------------------------------------------------ //
        // Inspector — robot list
        // ------------------------------------------------------------------ //

        /// <summary>
        /// All robots managed by this instance.  Populated automatically when
        /// <see cref="autoDiscoverRobots"/> is <c>true</c>, or assign manually.
        /// </summary>
        [Tooltip("Robots participating in the simulation.  Auto-populated when autoDiscoverRobots is true.")]
        public List<NmRobotBase> robots = new List<NmRobotBase>();

        /// <summary>
        /// When <c>true</c> the manager calls <c>FindObjectsOfType&lt;NmRobotBase&gt;()</c>
        /// on Start to populate <see cref="robots"/>.
        /// </summary>
        [Tooltip("Automatically find all NmRobotBase instances in the scene on Start.")]
        public bool autoDiscoverRobots = true;

        // ------------------------------------------------------------------ //
        // Inspector — spec string (display / validation only)
        // ------------------------------------------------------------------ //

        /// <summary>
        /// Human-readable robot population spec in the form
        /// <c>["celegans=1", "hexapod=2", "nao=1"]</c>.
        /// Parsed by <see cref="ParseRobotSpec"/> for display purposes only;
        /// actual robots must be present as prefab instances in the scene.
        /// </summary>
        [Tooltip("Population spec strings e.g. \"celegans=1\".  Informational — actual robots must exist as scene instances.")]
        public string[] robotSpec = System.Array.Empty<string>();

        // ------------------------------------------------------------------ //
        // Inspector — debug overlay
        // ------------------------------------------------------------------ //

        /// <summary>Toggles the OnGUI debug overlay.</summary>
        [Tooltip("Show the debug status overlay in play mode.")]
        public bool showDebugOverlay = true;

        private GUIStyle _overlayStyle;
        private GUIStyle _headerStyle;

        // ------------------------------------------------------------------ //
        // Runtime state (visible in inspector)
        // ------------------------------------------------------------------ //

        [Header("Runtime Status (read-only)")]

        [Tooltip("Number of robots currently connected.")]
        [SerializeField] private int _connectedCount;

        [Tooltip("Total number of brain steps across all robots this frame.")]
        [SerializeField] private int _totalSteps;

        // ------------------------------------------------------------------ //
        // Parsed spec (informational)
        // ------------------------------------------------------------------ //

        /// <summary>
        /// Parsed population entries from <see cref="robotSpec"/>.
        /// Each entry is (brainId, count).
        /// </summary>
        public IReadOnlyList<(string brainId, int count)> ParsedSpec
            => _parsedSpec.AsReadOnly();

        private readonly List<(string brainId, int count)> _parsedSpec
            = new List<(string, int)>();

        // ------------------------------------------------------------------ //
        // MonoBehaviour lifecycle
        // ------------------------------------------------------------------ //

        private void Awake()
        {
            if (Instance != null && Instance != this)
            {
                Debug.LogWarning("[NmSimulationManager] Duplicate instance — destroying self.", this);
                Destroy(gameObject);
                return;
            }
            Instance = this;
        }

        private void Start()
        {
            ParseRobotSpec();

            if (autoDiscoverRobots)
            {
                robots.Clear();
#if UNITY_2023_1_OR_NEWER
                robots.AddRange(FindObjectsByType<NmRobotBase>(FindObjectsSortMode.None));
#else
                robots.AddRange(FindObjectsOfType<NmRobotBase>());
#endif
                Debug.Log($"[NmSimulationManager] Auto-discovered {robots.Count} robot(s).");
            }
        }

        private void Update()
        {
            // Refresh inspector-visible stats each frame.
            int connected = 0;
            int steps = 0;
            foreach (var r in robots)
            {
                if (r == null) continue;
                if (r.IsConnected) connected++;
                steps += r.StepCount;
            }
            _connectedCount = connected;
            _totalSteps = steps;
        }

        private void OnDestroy()
        {
            if (Instance == this) Instance = null;
        }

        // ------------------------------------------------------------------ //
        // Public API
        // ------------------------------------------------------------------ //

        /// <summary>
        /// Parses <see cref="robotSpec"/> entries of the form <c>"brainId=count"</c>
        /// into <see cref="ParsedSpec"/>.  Invalid entries are skipped with a warning.
        /// This method does NOT spawn or remove robots — it is informational only.
        /// </summary>
        public void ParseRobotSpec()
        {
            _parsedSpec.Clear();
            if (robotSpec == null) return;

            foreach (string entry in robotSpec)
            {
                if (string.IsNullOrWhiteSpace(entry)) continue;

                int eq = entry.IndexOf('=');
                if (eq <= 0 || eq == entry.Length - 1)
                {
                    Debug.LogWarning($"[NmSimulationManager] Skipping malformed spec entry: '{entry}'. Expected 'brainId=count'.");
                    continue;
                }

                string id = entry.Substring(0, eq).Trim();
                string countStr = entry.Substring(eq + 1).Trim();

                if (!int.TryParse(countStr, out int count) || count < 0)
                {
                    Debug.LogWarning($"[NmSimulationManager] Skipping spec entry '{entry}': count must be a non-negative integer.");
                    continue;
                }

                _parsedSpec.Add((id, count));
            }
        }

        /// <summary>
        /// Adds a robot to the managed list if it is not already present.
        /// </summary>
        public void RegisterRobot(NmRobotBase robot)
        {
            if (robot != null && !robots.Contains(robot))
                robots.Add(robot);
        }

        /// <summary>
        /// Removes a robot from the managed list.
        /// </summary>
        public void UnregisterRobot(NmRobotBase robot)
        {
            robots.Remove(robot);
        }

        // ------------------------------------------------------------------ //
        // OnGUI overlay
        // ------------------------------------------------------------------ //

        private void OnGUI()
        {
            if (!showDebugOverlay) return;

            InitStyles();

            const float panelWidth = 320f;
            const float lineH = 20f;
            const float padX = 10f;
            const float padY = 10f;

            float rowCount = 3 + robots.Count; // header + summary + separator + per-robot
            float panelH = padY * 2 + rowCount * lineH;

            var rect = new Rect(padX, padY, panelWidth, panelH);
            GUI.Box(rect, GUIContent.none);

            float y = padY * 2;

            GUI.Label(new Rect(padX * 2, y, panelWidth - padX * 2, lineH),
                      "NeuralMimicry AARNN", _headerStyle);
            y += lineH;

            GUI.Label(new Rect(padX * 2, y, panelWidth - padX * 2, lineH),
                      $"Robots: {robots.Count}   Connected: {_connectedCount}   Steps: {_totalSteps}",
                      _overlayStyle);
            y += lineH;

            GUI.Label(new Rect(padX * 2, y, panelWidth - padX * 2, 1f),
                      "───────────────────────────", _overlayStyle);
            y += lineH;

            foreach (var r in robots)
            {
                if (r == null) continue;
                string status = r.IsConnected ? "<color=#00ff88>LIVE</color>" : "<color=#ff4444>DISC</color>";
                string connId = r.brainConnector != null ? r.brainConnector.brainId : "?";
                GUI.Label(new Rect(padX * 2, y, panelWidth - padX * 2, lineH),
                          $"{r.name}  [{connId}]  {status}  t={r.SimulationTimeMs:F0}ms",
                          _overlayStyle);
                y += lineH;
            }
        }

        private void InitStyles()
        {
            if (_overlayStyle != null) return;

            _overlayStyle = new GUIStyle(GUI.skin.label)
            {
                fontSize = 11,
                richText = true,
                normal = { textColor = Color.white },
            };

            _headerStyle = new GUIStyle(_overlayStyle)
            {
                fontSize = 13,
                fontStyle = FontStyle.Bold,
            };
        }
    }
}
