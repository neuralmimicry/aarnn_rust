const targetContainer = document.getElementById("targets");
const targetTemplate = document.getElementById("target-row");
const input = document.getElementById("addr");
const addButton = document.getElementById("add");
const networkSelect = document.getElementById("network-select");
const nodeSelect = document.getElementById("node-select");
const fullTopologyToggle = document.getElementById("full-topology");
const edgeLimitInput = document.getElementById("edge-limit");
const edgeLimitValue = document.getElementById("edge-limit-value");
const weightThresholdInput = document.getElementById("weight-threshold");
const weightThresholdValue = document.getElementById("weight-threshold-value");
const edgeCountEl = document.getElementById("edge-count");
const networkView = document.querySelector(".network-view");
const layoutButtons = document.querySelectorAll(".layout-toggle");
const canvas = document.getElementById("network-canvas");
const ctx = canvas.getContext("2d");
const startStopBtn = document.getElementById("start-stop");
const repeatBtn = document.getElementById("repeat");
const resetBtn = document.getElementById("reset");
const newBtn = document.getElementById("new");
const workspaceSelect = document.getElementById("workspace-select");
const workspaceUserInput = document.getElementById("workspace-user");
const workspaceNameInput = document.getElementById("workspace-name");
const workspaceRefreshBtn = document.getElementById("workspace-refresh");
const workspaceCreateBtn = document.getElementById("workspace-create");
const workspaceDeleteBtn = document.getElementById("workspace-delete");
const workspacePullBtn = document.getElementById("workspace-pull");
const workspacePushBtn = document.getElementById("workspace-push");
const workspaceStartBtn = document.getElementById("workspace-start");
const workspaceStopBtn = document.getElementById("workspace-stop");
const workspaceModeEl = document.getElementById("workspace-mode");
const workspaceStatusEl = document.getElementById("workspace-status");
const workspaceAutoscalerEl = document.getElementById("workspace-autoscaler");
const workspaceFeedbackEl = document.getElementById("workspace-feedback");

const cpuEl = document.getElementById("cpu");
const ramEl = document.getElementById("ram");
const tempEl = document.getElementById("temp");
const gpuEl = document.getElementById("gpu");
const gpuStatusEl = document.getElementById("gpu-status");
const neuronsEl = document.getElementById("neurons");
const depthStatusEl = document.getElementById("aarnn-depth-status");
const capacityScoreEl = document.getElementById("capacity-score");
const gaRunningEl = document.getElementById("ga-running");
const gaPacingEl = document.getElementById("ga-pacing");
const gaRampEl = document.getElementById("ga-ramp");
const gaProgressEl = document.getElementById("ga-progress");
const gaBestEl = document.getElementById("ga-best");
const clusterGaEvalsEl = document.getElementById("cluster-ga-evals");
const stepTimeEl = document.getElementById("step-time");
const activeTargetEl = document.getElementById("active-target");
const nodesCountEl = document.getElementById("nodes-count");
const networksCountEl = document.getElementById("networks-count");
const clusterNodesEl = document.getElementById("cluster-nodes");
const clusterNetworksEl = document.getElementById("cluster-networks");

const modelSelector = document.getElementById("model-selector");
const learningSelector = document.getElementById("learning-selector");
const aarnnRandomness = document.getElementById("aarnn-randomness");
const aarnnRandomnessValue = document.getElementById("aarnn-randomness-value");
const aarnnDepth = document.getElementById("aarnn-depth");
const aarnnDepthValue = document.getElementById("aarnn-depth-value");
const useDelays = document.getElementById("use-delays");
const useMorphology = document.getElementById("use-morphology");
const useStp = document.getElementById("use-stp");
const useNeuromod = document.getElementById("use-neuromod");
const resetBioBtn = document.getElementById("reset-bio");
const evolution3d = document.getElementById("evolution-3d");
const growth3dInput = document.getElementById("growth-3d");
const showRegionLabelsInput = document.getElementById("show-region-labels");
const clumpingDesign = document.getElementById("clumping-design");
const exportNeuromlBtn = document.getElementById("export-neuroml");
const exportPynnBtn = document.getElementById("export-pynn");
const exportNirBtn = document.getElementById("export-nir");
const exportOnnxBtn = document.getElementById("export-onnx");
const exportTfliteBtn = document.getElementById("export-tflite");
const ioInputSource = document.getElementById("io-input-source");
const ioInputUrl = document.getElementById("io-input-url");
const ioAerBase = document.getElementById("io-aer-base");
const ioSourceToggle = document.getElementById("io-source-toggle");
const ioSourceStatus = document.getElementById("io-source-status");
const authOverlay = document.getElementById("auth-overlay");
const authMessage = document.getElementById("auth-message");
const loginForm = document.getElementById("login-form");
const loginUsername = document.getElementById("login-username");
const loginPassword = document.getElementById("login-password");
const loginError = document.getElementById("login-error");
const signupBtn = document.getElementById("signup-btn");
const oidcLogin = document.getElementById("oidc-login");
const userStatus = document.getElementById("user-status");
const logoutBtn = document.getElementById("logout-btn");
const eqPanel = document.getElementById("eq-panel");
const eqEmpty = document.getElementById("eq-empty");
const scopeCanvas = document.getElementById("scope-canvas");
const scopeCtx = scopeCanvas ? scopeCanvas.getContext("2d") : null;
const scopeProbesEl = document.getElementById("scope-probes");
const probeCountEl = document.getElementById("probe-count");
const rasterCanvas = document.getElementById("raster-canvas");
const rasterCtx = rasterCanvas ? rasterCanvas.getContext("2d") : null;
const rasterFramesEl = document.getElementById("raster-frames");
const probeSourceInput = document.getElementById("probe-source");
const probeLayerInput = document.getElementById("probe-layer");
const probeIndexInput = document.getElementById("probe-index");
const addProbeBtn = document.getElementById("add-probe");
const clearProbesBtn = document.getElementById("clear-probes");
const probeHint = document.getElementById("probe-hint");
const saveConfigBtn = document.getElementById("save-config");
const loadConfigBtn = document.getElementById("load-config");
const saveNetworkBtn = document.getElementById("save-network");
const loadNetworkBtn = document.getElementById("load-network");
const saveProbesBtn = document.getElementById("save-probes");
const loadProbesBtn = document.getElementById("load-probes");
const toolStatusEl = document.getElementById("tool-status");
const graphContextMenu = document.getElementById("graph-context-menu");
const graphContextTitle = document.getElementById("graph-context-title");
const graphContextDetails = document.getElementById("graph-context-details");
const graphAddProbeBtn = document.getElementById("graph-add-probe");

const POLL_MS = 2000;
const ACTIVITY_POLL_MS = 200;
const SNAPSHOT_POLL_TICK_MS = 150;
const SNAPSHOT_POLL_PLAYING_MS = 350;
const SNAPSHOT_POLL_IDLE_MS = 1200;
const EQ_BANDS = 12;
const PROBE_HISTORY = 220;
const RASTER_HISTORY = 180;
const PROBE_COLORS = [
  "#71e0b1",
  "#ffd37a",
  "#7db8ff",
  "#ff9b7a",
  "#d4a8ff",
  "#9ce67a",
  "#ffcf99",
  "#8dd8ff",
];

let bootstrapRuntimeDefaultUser = "";

const state = {
  targets: loadTargets(),
  cards: new Map(),
  active: loadActive(),
  activeNetwork: loadActiveNetwork(),
  activeNodeId: loadActiveNode(),
  networksByTarget: new Map(),
  statusByTarget: new Map(),
  graph: null,
  snapshot: null,
  activity: null,
  lastNetworkId: "",
  playingOverride: new Map(),
  view: {
    zoom: 1,
    offsetX: 0,
    offsetY: 0,
    rotation: 0,
  },
  render: loadRenderSettings(),
  lastModel: "",
  lastLearning: "",
  regionLabelStates: new Map(),
  io: loadIoSettings(),
  authMode: "none",
  allowSignup: false,
  user: null,
  identity: null,
  userConfigEnabled: false,
  runtime: {
    workspaces: [],
    activeWorkspace: loadActiveWorkspace(),
    userId: loadRuntimeUser(),
    defaultUser: "",
    autoscaler: null,
    details: new Map(),
  },
  lastSnapshotPollAt: 0,
  snapshotFailures: 0,
  instrumentation: loadInstrumentationState(),
};

let snapshotFetchInFlight = false;
let snapshotFetchQueued = false;
let ioSourceRunner = null;
let runtimeStatusRequestSeq = 0;

let configSaveTimer = null;
let suppressUserConfigSave = false;

function parseAuthGroups(value) {
  if (!Array.isArray(value)) return [];
  const seen = new Set();
  return value
    .map((item) => String(item || "").trim().toLowerCase())
    .filter((item) => {
      if (!item || seen.has(item)) return false;
      seen.add(item);
      return true;
    });
}

function authActiveTeamLabel(value) {
  if (!value || typeof value !== "object") return "";
  return String(value.team_name || value.name || value.team_id || value.id || "").trim();
}

function normalizeAuthIdentity(payload) {
  if (!payload || payload.authenticated === false) return null;
  const username = String(payload.username || payload.user || "").trim();
  if (!username) return null;
  const role = String(payload.role || "user").trim().toLowerCase() || "user";
  const groups = parseAuthGroups(payload.groups);
  if (!groups.includes(role)) {
    groups.unshift(role);
  }
  const activeTeam = payload.active_team && typeof payload.active_team === "object" ? payload.active_team : null;
  const teamCount = Math.max(0, Number(payload.team_count ?? (activeTeam ? 1 : 0)) || 0);
  const pendingInvitationCount = Math.max(0, Number(payload.pending_invitation_count ?? 0) || 0);
  return {
    username,
    role,
    groups,
    email: payload.email ? String(payload.email).trim() : null,
    activeTeam,
    activeTeamLabel: authActiveTeamLabel(activeTeam),
    teamCount,
    pendingInvitationCount,
    isAdmin: Boolean(payload.is_admin || role === "admin" || groups.includes("admin")),
  };
}

function applyAuthIdentity(payload) {
  state.identity = normalizeAuthIdentity(payload);
  state.user = state.identity ? state.identity.username : null;
}

function userStatusLabel(identity) {
  if (!identity) return "Signed out";
  const parts = [`Signed in as ${identity.username}`];
  if (identity.groups.length) {
    parts.push(`groups: ${identity.groups.join(", ")}`);
  }
  if (identity.activeTeamLabel) {
    parts.push(`team: ${identity.activeTeamLabel}`);
  }
  if (identity.teamCount > 1) {
    parts.push(`teams: ${identity.teamCount}`);
  }
  if (identity.pendingInvitationCount > 0) {
    parts.push(`invites: ${identity.pendingInvitationCount}`);
  }
  return parts.join(" | ");
}

function probeDefaultLabel(targetType, layer, index) {
  if (targetType === "hidden") {
    return `H${layer + 1}:${index} spike`;
  }
  if (targetType === "output") {
    return `O${index} spike`;
  }
  return `S${index} spike`;
}

function normalizeProbe(raw, fallbackId = 1) {
  const targetType =
    raw && typeof raw.targetType === "string" && ["sensory", "hidden", "output"].includes(raw.targetType)
      ? raw.targetType
      : "sensory";
  const layer = targetType === "hidden" ? Math.max(0, Math.trunc(Number(raw?.layer || 0))) : 0;
  const index = Math.max(0, Math.trunc(Number(raw?.index || 0)));
  const id = Math.max(1, Math.trunc(Number(raw?.id || fallbackId)));
  return {
    id,
    targetType,
    layer,
    index,
    label:
      raw && typeof raw.label === "string" && raw.label.trim()
        ? raw.label.trim()
        : probeDefaultLabel(targetType, layer, index),
    color:
      raw && typeof raw.color === "string" && raw.color.trim()
        ? raw.color.trim()
        : PROBE_COLORS[(id - 1) % PROBE_COLORS.length],
    enabled: raw?.enabled !== false,
    samples: [],
  };
}

function serializeProbe(probe) {
  return {
    id: probe.id,
    targetType: probe.targetType,
    layer: probe.targetType === "hidden" ? probe.layer : 0,
    index: probe.index,
    label: probe.label,
    color: probe.color,
    enabled: probe.enabled !== false,
  };
}

function serializeProbes() {
  return (state.instrumentation?.probes || []).map(serializeProbe);
}

function loadInstrumentationState() {
  let probes = [];
  try {
    const raw = localStorage.getItem("nm_instrumentation");
    if (raw) {
      const parsed = JSON.parse(raw);
      if (Array.isArray(parsed?.probes)) {
        probes = parsed.probes.map((probe, idx) => normalizeProbe(probe, idx + 1));
      }
    }
  } catch (_) {}
  const nextProbeId = probes.reduce((maxId, probe) => Math.max(maxId, probe.id), 0) + 1;
  return {
    probes,
    nextProbeId,
    eqBands: Array.from({ length: EQ_BANDS }, () => 0),
    outputRaster: [],
    screenNodes: [],
    contextTarget: null,
  };
}

function saveInstrumentationState() {
  const payload = {
    probes: serializeProbes(),
  };
  if (state.userConfigEnabled) {
    scheduleUserConfigSave();
    return;
  }
  localStorage.setItem("nm_instrumentation", JSON.stringify(payload));
}

function resetInstrumentationBuffers({ keepProbes = true } = {}) {
  state.instrumentation.eqBands = Array.from({ length: EQ_BANDS }, () => 0);
  state.instrumentation.outputRaster = [];
  state.instrumentation.screenNodes = [];
  state.instrumentation.contextTarget = null;
  if (keepProbes) {
    state.instrumentation.probes.forEach((probe) => {
      probe.samples = [];
    });
  } else {
    state.instrumentation.probes = [];
    state.instrumentation.nextProbeId = 1;
    saveInstrumentationState();
  }
}

function buildUserConfig() {
  const ioConfig = {
    sourceType: state.io.sourceType === "aer-http-stream" ? "aer-http-stream" : "none",
    sourceUrl: typeof state.io.sourceUrl === "string" ? state.io.sourceUrl : "",
    aerBase: Number.isFinite(Number(state.io.aerBase)) ? Math.max(0, Math.trunc(Number(state.io.aerBase))) : 0,
  };
  return {
    targets: state.targets,
    active: state.active,
    activeNetwork: state.activeNetwork,
    activeNode: state.activeNodeId,
    activeWorkspace: state.runtime.activeWorkspace,
    render: state.render,
    io: ioConfig,
    instrumentation: {
      probes: serializeProbes(),
    },
  };
}

function scheduleUserConfigSave() {
  if (!state.userConfigEnabled || suppressUserConfigSave) return;
  if (configSaveTimer) clearTimeout(configSaveTimer);
  configSaveTimer = setTimeout(saveUserConfigNow, 300);
}

async function saveUserConfigNow() {
  if (!state.userConfigEnabled) return;
  try {
    await fetch("/api/user/config", {
      method: "POST",
      headers: { "Content-Type": "application/json" },
      body: JSON.stringify({ config: buildUserConfig() }),
    });
  } catch (e) {
    console.warn("Failed to save user config", e);
  }
}

function applyUserConfig(cfg) {
  if (!cfg || typeof cfg !== "object") return;
  suppressUserConfigSave = true;
  if (Array.isArray(cfg.targets)) state.targets = cfg.targets;
  if (typeof cfg.active === "string") state.active = cfg.active;
  if (typeof cfg.activeNetwork === "string") state.activeNetwork = cfg.activeNetwork;
  if (typeof cfg.activeNode === "string") state.activeNodeId = cfg.activeNode;
  if (typeof cfg.activeWorkspace === "string") state.runtime.activeWorkspace = cfg.activeWorkspace;
  if (cfg.render && typeof cfg.render === "object") {
    state.render = {
      ...loadRenderSettings(),
      ...cfg.render,
    };
  }
  if (cfg.io && typeof cfg.io === "object") {
    state.io = {
      ...loadIoSettings(),
      ...cfg.io,
    };
  }
  if (cfg.instrumentation && typeof cfg.instrumentation === "object") {
    const incoming = Array.isArray(cfg.instrumentation.probes) ? cfg.instrumentation.probes : [];
    state.instrumentation.probes = incoming.map((probe, idx) => normalizeProbe(probe, idx + 1));
    state.instrumentation.nextProbeId =
      state.instrumentation.probes.reduce((maxId, probe) => Math.max(maxId, probe.id), 0) + 1;
    resetInstrumentationBuffers();
  }
  renderInstrumentation();
  suppressUserConfigSave = false;
}

function loadTargets() {
  try {
    const raw = localStorage.getItem("nm_targets");
    if (!raw) return [];
    const parsed = JSON.parse(raw);
    if (Array.isArray(parsed)) return parsed;
  } catch (_) {}
  return [];
}

function saveTargets() {
  if (state.userConfigEnabled) {
    scheduleUserConfigSave();
    return;
  }
  localStorage.setItem("nm_targets", JSON.stringify(state.targets));
}

function loadActive() {
  try {
    return localStorage.getItem("nm_active") || "";
  } catch (_) {
    return "";
  }
}

function saveActive() {
  if (state.userConfigEnabled) {
    scheduleUserConfigSave();
    return;
  }
  try {
    localStorage.setItem("nm_active", state.active);
  } catch (_) {}
}

function loadActiveNetwork() {
  try {
    return localStorage.getItem("nm_active_network") || "";
  } catch (_) {
    return "";
  }
}

function saveActiveNetwork() {
  if (state.userConfigEnabled) {
    scheduleUserConfigSave();
    return;
  }
  try {
    localStorage.setItem("nm_active_network", state.activeNetwork);
  } catch (_) {}
}

function loadActiveNode() {
  try {
    return localStorage.getItem("nm_active_node") || "";
  } catch (_) {
    return "";
  }
}

function loadActiveWorkspace() {
  try {
    return localStorage.getItem("nm_active_workspace") || "";
  } catch (_) {
    return "";
  }
}

function isGeneratedRuntimeUser(value) {
  return /^web-[a-z0-9]{8}$/i.test((value || "").trim());
}

function defaultRuntimeUser() {
  const configured = bootstrapRuntimeDefaultUser.trim();
  if (configured) {
    return configured;
  }
  return `web-${Math.random().toString(36).slice(2, 10)}`;
}

function loadRuntimeUser() {
  try {
    const existing = (localStorage.getItem("nm_runtime_user") || "").trim();
    if (existing) {
      return existing;
    }
    const generated = defaultRuntimeUser();
    localStorage.setItem("nm_runtime_user", generated);
    return generated;
  } catch (_) {
    return defaultRuntimeUser();
  }
}

function saveRuntimeUser() {
  try {
    localStorage.setItem("nm_runtime_user", (state.runtime.userId || "").trim());
  } catch (_) {}
}

function saveActiveWorkspace() {
  if (state.userConfigEnabled) {
    scheduleUserConfigSave();
    return;
  }
  try {
    localStorage.setItem("nm_active_workspace", state.runtime.activeWorkspace || "");
  } catch (_) {}
}

function applyBootstrapConfig(cfg = {}) {
  const defaultUser =
    cfg && typeof cfg.default_runtime_user === "string"
      ? cfg.default_runtime_user.trim()
      : "";
  bootstrapRuntimeDefaultUser = defaultUser;
  state.runtime.defaultUser = defaultUser;
  if (state.authMode === "none" && defaultUser) {
    const current = (state.runtime.userId || "").trim();
    if (!current || isGeneratedRuntimeUser(current)) {
      state.runtime.userId = defaultUser;
      saveRuntimeUser();
    }
  }
}

async function loadBootstrapConfig() {
  try {
    const res = await fetch("/api/config");
    if (!res.ok) return null;
    const cfg = await res.json();
    applyBootstrapConfig(cfg);
    return cfg;
  } catch (_) {
    return null;
  }
}

function runtimeUserLabel() {
  if (state.authMode === "none") {
    return (state.runtime.userId || "").trim() || "anonymous";
  }
  return state.user || "authenticated";
}

function clusterModeAllowed() {
  return state.authMode === "none";
}

function runtimeFetch(path, options = {}) {
  const headers = new Headers(options.headers || {});
  if (state.authMode === "none") {
    const runtimeUser = (state.runtime.userId || "").trim();
    if (runtimeUser) {
      headers.set("x-nm-runtime-user", runtimeUser);
    }
  }
  return fetch(path, {
    ...options,
    headers,
  });
}

function saveActiveNode() {
  if (state.userConfigEnabled) {
    scheduleUserConfigSave();
    return;
  }
  try {
    localStorage.setItem("nm_active_node", state.activeNodeId);
  } catch (_) {}
}

function loadRenderSettings() {
  try {
    const raw = localStorage.getItem("nm_render");
    if (!raw) throw new Error("missing");
    const parsed = JSON.parse(raw);
    return {
      fullTopology: Boolean(parsed.fullTopology),
      edgeLimit: Number(parsed.edgeLimit || 6000),
      weightThreshold: Number(parsed.weightThreshold || 0.0),
      layout: parsed.layout === "conventional" ? "conventional" : "aarnn",
      showRegionLabels: parsed.showRegionLabels !== undefined ? Boolean(parsed.showRegionLabels) : true,
    };
  } catch (_) {
    return {
      fullTopology: false,
      edgeLimit: 6000,
      weightThreshold: 0.0,
      layout: "aarnn",
      showRegionLabels: true,
    };
  }
}

function loadIoSettings() {
  try {
    const raw = localStorage.getItem("nm_io");
    if (!raw) throw new Error("missing");
    const parsed = JSON.parse(raw);
    return {
      sourceType: parsed.sourceType === "aer-http-stream" ? "aer-http-stream" : "none",
      sourceUrl: typeof parsed.sourceUrl === "string" ? parsed.sourceUrl : "",
      aerBase: Number.isFinite(Number(parsed.aerBase)) ? Math.max(0, Number(parsed.aerBase)) : 0,
      streaming: false,
      status: "Disconnected",
      statusClass: "io-status-idle",
      connectedAt: 0,
      defaultAddr: "",
      defaultNetworkId: "",
    };
  } catch (_) {
    return {
      sourceType: "none",
      sourceUrl: "",
      aerBase: 0,
      streaming: false,
      status: "Disconnected",
      statusClass: "io-status-idle",
      connectedAt: 0,
      defaultAddr: "",
      defaultNetworkId: "",
    };
  }
}

function saveIoSettings() {
  const payload = {
    sourceType: state.io.sourceType === "aer-http-stream" ? "aer-http-stream" : "none",
    sourceUrl: typeof state.io.sourceUrl === "string" ? state.io.sourceUrl.trim() : "",
    aerBase: Number.isFinite(Number(state.io.aerBase)) ? Math.max(0, Math.trunc(Number(state.io.aerBase))) : 0,
  };
  state.io.sourceType = payload.sourceType;
  state.io.sourceUrl = payload.sourceUrl;
  state.io.aerBase = payload.aerBase;
  if (state.userConfigEnabled) {
    scheduleUserConfigSave();
    return;
  }
  localStorage.setItem("nm_io", JSON.stringify(payload));
}

function saveRenderSettings() {
  if (state.userConfigEnabled) {
    scheduleUserConfigSave();
    return;
  }
  localStorage.setItem("nm_render", JSON.stringify(state.render));
}

async function initAuth() {
  await loadBootstrapConfig();
  try {
    const modeResp = await fetch("/api/auth/mode");
    if (modeResp.ok) {
      const data = await modeResp.json();
      state.authMode = data.mode || "none";
      state.allowSignup = Boolean(data.allow_signup);
    }
  } catch (_) {
    state.authMode = "none";
  }

  if (state.authMode === "none") {
    applyAuthIdentity(null);
    setUserStatus(state.identity);
    hideAuthOverlay();
    return;
  }

  const meResp = await fetch("/api/me");
  if (meResp.ok) {
    const data = await meResp.json();
    applyAuthIdentity(data);
    state.userConfigEnabled = Boolean(state.user);
    setUserStatus(state.identity);
    await loadUserConfig();
    await loadRuntimeStatus();
    hideAuthOverlay();
  } else {
    applyAuthIdentity(null);
    state.userConfigEnabled = false;
    showAuthOverlay();
  }
}

async function loadUserConfig() {
  try {
    const resp = await fetch("/api/user/config");
    if (!resp.ok) return;
    const data = await resp.json();
    applyUserConfig(data.config || {});
  } catch (e) {
    console.warn("Failed to load user config", e);
  }
}

function showAuthOverlay() {
  if (!authOverlay) return;
  authOverlay.classList.remove("hidden");
  if (loginError) loginError.textContent = "";
  if (authMessage) {
    authMessage.textContent = state.authMode === "oidc" ? "Continue with your SSO provider." : "Enter your credentials.";
  }
  if (loginForm) {
    loginForm.style.display = state.authMode === "local" ? "flex" : "none";
  }
  if (oidcLogin) {
    oidcLogin.style.display = state.authMode === "oidc" ? "inline-flex" : "none";
  }
  if (signupBtn) {
    signupBtn.style.display = state.allowSignup ? "inline-flex" : "none";
  }
}

function hideAuthOverlay() {
  if (!authOverlay) return;
  authOverlay.classList.add("hidden");
}

function setUserStatus(identity) {
  if (!userStatus) return;
  userStatus.textContent = userStatusLabel(identity);
  if (logoutBtn) {
    logoutBtn.style.display = identity ? "inline-flex" : "none";
  }
}

async function performLogin(username, password) {
  if (!username || !password) {
    showAuthError("Enter username and password.");
    return;
  }
  try {
    const resp = await fetch("/api/login", {
      method: "POST",
      headers: { "Content-Type": "application/json" },
      body: JSON.stringify({ username, password }),
    });
    if (!resp.ok) {
      const data = await resp.json().catch(() => ({}));
      showAuthError(data.error || "Login failed.");
      return;
    }
    const data = await resp.json();
    applyAuthIdentity(data && typeof data === "object" ? data : { username });
    state.userConfigEnabled = Boolean(state.user);
    setUserStatus(state.identity);
    await loadUserConfig();
    await loadRuntimeStatus();
    resetTargetsUi();
    await initTargets();
    refreshNetworkSelect();
    if (isWorkspaceMode()) {
      await fetchSnapshotForActive();
      await pollActivity();
    }
    syncRenderControls();
    syncIoControls();
    renderInstrumentation();
    hideAuthOverlay();
  } catch (e) {
    showAuthError("Login failed.");
  }
}

async function performSignup(username, password) {
  if (!username || !password) {
    showAuthError("Enter username and password.");
    return;
  }
  try {
    const resp = await fetch("/api/signup", {
      method: "POST",
      headers: { "Content-Type": "application/json" },
      body: JSON.stringify({ username, password }),
    });
    if (!resp.ok) {
      const data = await resp.json().catch(() => ({}));
      showAuthError(data.error || "Signup failed.");
      return;
    }
    showAuthError("Signup successful. Please log in.");
  } catch (e) {
    showAuthError("Signup failed.");
  }
}

async function performLogout() {
  if (state.io.streaming) {
    stopIoSourceStream();
  }
  try {
    await fetch("/api/logout", { method: "POST" });
  } catch (_) {}
  applyAuthIdentity(null);
  state.userConfigEnabled = false;
  state.targets = [];
  state.active = "";
  state.activeNetwork = "";
  state.activeNodeId = "";
  state.statusByTarget.clear();
  state.networksByTarget.clear();
  state.runtime.workspaces = [];
  state.runtime.details.clear();
  state.runtime.autoscaler = null;
  state.snapshot = null;
  state.activity = null;
  state.graph = null;
  resetTargetsUi();
  refreshWorkspaceSelect();
  setPlaceholder();
  drawNetwork();
  setUserStatus(state.identity);
  if (state.authMode !== "none") {
    showAuthOverlay();
  }
}

function showAuthError(message) {
  if (!loginError) return;
  loginError.textContent = message;
}

function resetTargetsUi() {
  state.cards.forEach((card) => card.node.remove());
  state.cards.clear();
  targetContainer.innerHTML = "";
}

function normalizeAddr(addr) {
  if (!addr) return "";
  if (!addr.startsWith("http://") && !addr.startsWith("https://")) {
    return `http://${addr}`;
  }
  return addr;
}

function statusHealthScore(status) {
  if (!status || typeof status !== "object") return 0;
  const nodes = Array.isArray(status.nodes) ? status.nodes.length : 0;
  const networks = Array.isArray(status.networks) ? status.networks.length : 0;
  return nodes * 100 + networks;
}

function nodeIdentity(node) {
  if (!node || typeof node !== "object") return "";
  if (node.node_id) return `id:${node.node_id}`;
  if (node.address) return `addr:${node.address}`;
  return "";
}

function mergeDistributions(base, incoming) {
  const merged = new Map();
  (Array.isArray(base) ? base : []).forEach((entry) => {
    if (!entry || !entry.node_id) return;
    merged.set(entry.node_id, entry);
  });
  (Array.isArray(incoming) ? incoming : []).forEach((entry) => {
    if (!entry || !entry.node_id) return;
    const current = merged.get(entry.node_id);
    if (!current) {
      merged.set(entry.node_id, entry);
      return;
    }
    const currentLayers = Array.isArray(current.layers) ? current.layers.length : 0;
    const nextLayers = Array.isArray(entry.layers) ? entry.layers.length : 0;
    if (nextLayers > currentLayers) {
      merged.set(entry.node_id, entry);
    }
  });
  return Array.from(merged.values());
}

function aggregateClusterStatus() {
  const nodesById = new Map();
  const networksById = new Map();

  state.statusByTarget.forEach((status) => {
    const nodes = Array.isArray(status?.nodes) ? status.nodes : [];
    nodes.forEach((node) => {
      const key = nodeIdentity(node);
      if (!key) return;
      const current = nodesById.get(key);
      if (!current) {
        nodesById.set(key, node);
        return;
      }
      const currentNetworks = Array.isArray(current.active_networks) ? current.active_networks.length : 0;
      const nextNetworks = Array.isArray(node.active_networks) ? node.active_networks.length : 0;
      if (nextNetworks > currentNetworks || Number(node.capacity_score || 0) > Number(current.capacity_score || 0)) {
        nodesById.set(key, node);
      }
    });

    const networks = Array.isArray(status?.networks) ? status.networks : [];
    networks.forEach((net) => {
      if (!net || !net.network_id) return;
      const current = networksById.get(net.network_id);
      if (!current) {
        networksById.set(net.network_id, net);
        return;
      }
      const merged = {
        ...current,
        ...net,
      };
      merged.playing = Boolean(current.playing) || Boolean(net.playing);
      merged.total_neurons = Math.max(Number(current.total_neurons || 0), Number(net.total_neurons || 0));
      merged.num_layers = Math.max(Number(current.num_layers || 0), Number(net.num_layers || 0));
      merged.desired_aarnn_depth = Math.max(
        Number(current.desired_aarnn_depth || 0),
        Number(net.desired_aarnn_depth || 0)
      );
      merged.distribution = mergeDistributions(current.distribution, net.distribution);
      networksById.set(net.network_id, merged);
    });
  });

  const nodes = Array.from(nodesById.values()).sort((a, b) =>
    (a.node_id || a.address || "").localeCompare(b.node_id || b.address || "")
  );
  const networks = Array.from(networksById.values()).sort((a, b) =>
    (a.network_id || "").localeCompare(b.network_id || "")
  );

  return { nodes, networks };
}

function isWorkspaceMode() {
  return !clusterModeAllowed() || Boolean(state.runtime.activeWorkspace);
}

function getActiveWorkspaceMeta() {
  return (
    state.runtime.workspaces.find(
      (workspace) => workspace.workspace_id === state.runtime.activeWorkspace
    ) || null
  );
}

function getActiveWorkspaceDetail() {
  return state.runtime.details.get(state.runtime.activeWorkspace) || null;
}

function cacheWorkspaceDetail(detail) {
  const workspaceId = detail?.summary?.workspace_id;
  if (!workspaceId) return;
  state.runtime.details.set(workspaceId, detail);
  const idx = state.runtime.workspaces.findIndex(
    (workspace) => workspace.workspace_id === workspaceId
  );
  if (idx >= 0) {
    state.runtime.workspaces[idx] = {
      ...state.runtime.workspaces[idx],
      ...detail.summary,
    };
  } else {
    state.runtime.workspaces.push(detail.summary);
  }
}

async function loadWorkspaceDetail(workspaceId = state.runtime.activeWorkspace) {
  if (!workspaceId) return null;
  try {
    const resp = await runtimeFetch(
      `/api/runtime/workspaces/${encodeURIComponent(workspaceId)}`
    );
    if (!resp.ok) {
      if (resp.status === 404) {
        state.runtime.details.delete(workspaceId);
      }
      return null;
    }
    const detail = await resp.json();
    cacheWorkspaceDetail(detail);
    return detail;
  } catch (_) {
    return null;
  }
}

function activeSource() {
  if (isWorkspaceMode()) {
    const workspace = getActiveWorkspaceMeta();
    if (workspace) {
      return {
        kind: "workspace",
        workspace,
        networkId: workspace.network_id || workspace.workspace_id,
      };
    }
  }
  if (state.active && state.activeNetwork) {
    return {
      kind: "cluster",
      addr: state.active,
      networkId: state.activeNetwork,
      nodeId: state.activeNodeId || "",
    };
  }
  return null;
}

function sourceRequestKey(source) {
  if (!source) return "";
  if (source.kind === "workspace") {
    return `workspace::${source.workspace.workspace_id}`;
  }
  return `${source.addr}::${source.networkId}::${source.nodeId || ""}`;
}

function workspaceNetworkMeta(workspace, detail = getActiveWorkspaceDetail()) {
  if (!workspace) return null;
  const status = detail?.status || {};
  const hiddenLayers = Number(
    workspace.num_hidden_layers ??
      status.num_hidden_layers ??
      state.snapshot?.net?.num_hidden_layers ??
      0
  );
  const desiredDepth = Number(
    workspace.desired_aarnn_depth ??
      status.desired_aarnn_depth ??
      state.snapshot?.net?.aarnn_layer_depth ??
      0
  );
  return {
    network_id: workspace.network_id || workspace.workspace_id,
    playing: Boolean(workspace.running),
    total_neurons: Number(workspace.total_neurons ?? status.total_neurons ?? 0),
    num_layers: hiddenLayers + 1,
    desired_aarnn_depth: desiredDepth,
    neuron_model: status.neuron_model || state.lastModel || "aarnn",
    learning_rule: status.learning_rule || state.lastLearning || "aarnn",
  };
}

function refreshWorkspaceSelect() {
  if (!workspaceSelect) return;
  workspaceSelect.innerHTML = "";
  if (clusterModeAllowed()) {
    const clusterOpt = document.createElement("option");
    clusterOpt.value = "";
    clusterOpt.textContent = "Cluster / orchestrator mode";
    workspaceSelect.appendChild(clusterOpt);
  }

  state.runtime.workspaces.forEach((workspace) => {
    const opt = document.createElement("option");
    opt.value = workspace.workspace_id;
    opt.textContent = `${workspace.name || workspace.workspace_id}${
      workspace.running ? " (running)" : ""
    }`;
    workspaceSelect.appendChild(opt);
  });

  if (
    state.runtime.activeWorkspace &&
    !state.runtime.workspaces.some(
      (workspace) => workspace.workspace_id === state.runtime.activeWorkspace
    )
  ) {
    state.runtime.activeWorkspace = "";
    saveActiveWorkspace();
  }
  workspaceSelect.value = state.runtime.activeWorkspace || "";
  syncWorkspaceUi();
}

function syncWorkspaceUi() {
  const workspace = getActiveWorkspaceMeta();
  const detail = getActiveWorkspaceDetail();
  const running = workspace ? Boolean(detail?.summary?.running ?? workspace.running) : false;
  const simTimeMs = Number(detail?.status?.sim_time_ms ?? workspace?.sim_time_ms ?? 0);
  const step = Number(detail?.status?.step ?? workspace?.step ?? 0);
  if (workspaceModeEl) {
    workspaceModeEl.textContent = workspace || !clusterModeAllowed() ? "workspace" : "cluster";
  }
  if (workspaceUserInput) {
    if (document.activeElement !== workspaceUserInput) {
      workspaceUserInput.value = runtimeUserLabel();
    }
    workspaceUserInput.disabled = state.authMode !== "none";
  }
  if (workspaceStatusEl) {
    workspaceStatusEl.textContent = workspace
      ? `${running ? "running" : "stopped"} | t=${simTimeMs.toFixed(1)} ms | step ${step}`
      : "inactive";
  }
  if (workspaceAutoscalerEl) {
    const autoscaler = state.runtime.autoscaler || {};
    workspaceAutoscalerEl.textContent = autoscaler.provider
      ? `${autoscaler.provider}${
          Number(autoscaler.active_remote_nodes || 0) > 0
            ? ` | remote ${Number(autoscaler.active_remote_nodes || 0)}`
            : ""
        }${autoscaler.last_action ? ` | ${autoscaler.last_action}` : ""}`
      : "local";
  }
  if (input) input.disabled = !clusterModeAllowed();
  if (addButton) addButton.disabled = !clusterModeAllowed();
  if (workspaceDeleteBtn) workspaceDeleteBtn.disabled = !workspace;
  if (workspacePullBtn) workspacePullBtn.disabled = !workspace;
  if (workspacePushBtn) workspacePushBtn.disabled = !workspace || !(currentNetworkJson() || currentConfigJson());
  if (workspaceStartBtn) workspaceStartBtn.disabled = !workspace || running;
  if (workspaceStopBtn) workspaceStopBtn.disabled = !workspace || !running;
  if (networkSelect) networkSelect.disabled = Boolean(workspace) || !clusterModeAllowed();
  if (nodeSelect) nodeSelect.disabled = Boolean(workspace) || !clusterModeAllowed();
}

async function loadRuntimeStatus() {
  if (state.authMode !== "none" && !state.user) return;
  const requestSeq = ++runtimeStatusRequestSeq;
  try {
    const resp = await runtimeFetch("/api/runtime/status");
    // Ignore stale responses so an older poll cannot wipe a newer workspace list.
    if (requestSeq !== runtimeStatusRequestSeq) return;
    if (!resp.ok) {
      refreshWorkspaceSelect();
      return;
    }
    const data = await resp.json();
    if (requestSeq !== runtimeStatusRequestSeq) return;
    state.runtime.workspaces = Array.isArray(data.workspaces) ? data.workspaces : [];
    state.runtime.autoscaler = data.autoscaler || null;
    const activeIds = new Set(state.runtime.workspaces.map((workspace) => workspace.workspace_id));
    Array.from(state.runtime.details.keys()).forEach((workspaceId) => {
      if (!activeIds.has(workspaceId)) {
        state.runtime.details.delete(workspaceId);
      }
    });
    if (!clusterModeAllowed()) {
      if (!state.runtime.activeWorkspace || !activeIds.has(state.runtime.activeWorkspace)) {
        state.runtime.activeWorkspace = state.runtime.workspaces[0]?.workspace_id || "";
        saveActiveWorkspace();
      }
    }
    if (state.runtime.activeWorkspace) {
      await loadWorkspaceDetail(state.runtime.activeWorkspace);
    }
    refreshWorkspaceSelect();
    if (isWorkspaceMode()) {
      renderWorkspaceSidebar();
      refreshControlButtons();
    }
  } catch (_) {
    if (requestSeq !== runtimeStatusRequestSeq) return;
    refreshWorkspaceSelect();
  }
}

async function createWorkspaceFromCurrentState() {
  const name = workspaceNameInput ? workspaceNameInput.value.trim() : "";
  const snapshotJson = currentNetworkJson();
  const configJson = currentConfigJson();
  const activeModel = modelSelector.querySelector("button.active")?.dataset.model;
  const activeLearning = learningSelector.querySelector("button.active")?.dataset.learning;
  const payload = {
    workspace_id: name ? name.toLowerCase().replace(/[^a-z0-9_.-]+/g, "-") : undefined,
    name: name || undefined,
    snapshot_json: snapshotJson || undefined,
    config_json: snapshotJson ? undefined : configJson || undefined,
    neuron_model: activeModel,
    learning_rule: activeLearning,
  };
  try {
    const resp = await runtimeFetch("/api/runtime/workspaces", {
      method: "POST",
      headers: { "Content-Type": "application/json" },
      body: JSON.stringify(payload),
    });
    if (!resp.ok) {
      const err = await resp.json().catch(() => ({}));
      const message = formatWorkspaceApiError(err, "Failed to create workspace.");
      setWorkspaceFeedback(message, "error");
      setToolStatus(message);
      return;
    }
    const detail = await resp.json();
    cacheWorkspaceDetail(detail);
    state.runtime.activeWorkspace = detail?.summary?.workspace_id || payload.workspace_id || "";
    saveActiveWorkspace();
    refreshWorkspaceSelect();
    if (workspaceNameInput) workspaceNameInput.value = "";
    await loadRuntimeStatus();
    refreshNetworkSelect();
    await fetchSnapshotForActive();
    await pollActivity();
    const message = `Created workspace ${state.runtime.activeWorkspace}.`;
    setWorkspaceFeedback(message, "success");
    setToolStatus(message);
  } catch (_) {
    setWorkspaceFeedback("Failed to create workspace.", "error");
    setToolStatus("Failed to create workspace.");
  }
}

async function deleteSelectedWorkspace() {
  const workspace = getActiveWorkspaceMeta();
  if (!workspace) return;
  try {
    const resp = await runtimeFetch(
      `/api/runtime/workspaces/${encodeURIComponent(workspace.workspace_id)}`,
      { method: "DELETE" }
    );
    if (!resp.ok) {
      const err = await resp.json().catch(() => ({}));
      const message = formatWorkspaceApiError(err, "Failed to delete workspace.");
      setWorkspaceFeedback(message, "error");
      setToolStatus(message);
      return;
    }
    state.runtime.details.delete(workspace.workspace_id);
    state.runtime.activeWorkspace = "";
    saveActiveWorkspace();
    state.snapshot = null;
    state.activity = null;
    await loadRuntimeStatus();
    refreshNetworkSelect();
    await pollAll();
    if (!isWorkspaceMode()) {
      await fetchSnapshotForActive();
      await pollActivity();
    } else {
      rebuildGraph();
    }
    const message = `Deleted workspace ${workspace.workspace_id}.`;
    setWorkspaceFeedback(message, "success");
    setToolStatus(message);
  } catch (_) {
    setWorkspaceFeedback("Failed to delete workspace.", "error");
    setToolStatus("Failed to delete workspace.");
  }
}

async function importWorkspacePayload(raw, kind, extra = {}) {
  const workspace = getActiveWorkspaceMeta();
  if (!workspace) {
    setWorkspaceFeedback("Select a workspace first.", "error");
    setToolStatus("Select a workspace first.");
    return false;
  }
  try {
    const resp = await runtimeFetch(
      `/api/runtime/workspaces/${encodeURIComponent(workspace.workspace_id)}/import`,
      {
        method: "POST",
        headers: { "Content-Type": "application/json" },
        body: JSON.stringify({
          payload_json: raw,
          kind,
          replace_baseline: Boolean(extra.replaceBaseline),
          auto_start: Boolean(extra.autoStart),
          neuron_model: extra.neuron_model,
          learning_rule: extra.learning_rule,
        }),
      }
    );
    if (!resp.ok) {
      const err = await resp.json().catch(() => ({}));
      const message = formatWorkspaceApiError(err, "Failed to update workspace.");
      setWorkspaceFeedback(message, "error");
      setToolStatus(message);
      return false;
    }
    const detail = await resp.json();
    cacheWorkspaceDetail(detail);
    await loadRuntimeStatus();
    await fetchSnapshotForActive();
    await pollActivity();
    setWorkspaceFeedback(`Updated workspace ${workspace.workspace_id}.`, "success");
    return true;
  } catch (_) {
    setWorkspaceFeedback("Failed to update workspace.", "error");
    setToolStatus("Failed to update workspace.");
    return false;
  }
}

async function controlWorkspaceAction(action) {
  const workspace = getActiveWorkspaceMeta();
  if (!workspace) return false;
  try {
    const resp = await runtimeFetch(
      `/api/runtime/workspaces/${encodeURIComponent(workspace.workspace_id)}/control`,
      {
        method: "POST",
        headers: { "Content-Type": "application/json" },
        body: JSON.stringify({ action }),
      }
    );
    if (!resp.ok) {
      const err = await resp.json().catch(() => ({}));
      const message = formatWorkspaceApiError(err, "Failed to control workspace.");
      setWorkspaceFeedback(message, "error");
      setToolStatus(message);
      return false;
    }
    const detail = await resp.json();
    cacheWorkspaceDetail(detail);
    await loadRuntimeStatus();
    await fetchSnapshotForActive();
    await pollActivity();
    setWorkspaceFeedback(`Workspace ${workspace.workspace_id} ${action}.`, "success");
    return true;
  } catch (_) {
    setWorkspaceFeedback("Failed to control workspace.", "error");
    setToolStatus("Failed to control workspace.");
    return false;
  }
}

function ensureCard(addr) {
  if (state.cards.has(addr)) return state.cards.get(addr);
  const node = targetTemplate.content.firstElementChild.cloneNode(true);
  const btn = node.querySelector(".target-btn");
  const remove = node.querySelector(".target-remove");
  btn.textContent = addr;
  btn.addEventListener("click", () => setActive(addr));
  remove.addEventListener("click", () => removeTarget(addr));
  targetContainer.appendChild(node);
  state.cards.set(addr, { node, btn });
  return state.cards.get(addr);
}

function removeTarget(addr) {
  state.targets = state.targets.filter((t) => t !== addr);
  saveTargets();
  const card = state.cards.get(addr);
  if (card) {
    card.node.remove();
    state.cards.delete(addr);
  }
  if (state.active === addr) {
    state.active = state.targets[0] || "";
    saveActive();
  }
}

function addTarget(addr) {
  const normalized = normalizeAddr(addr.trim());
  if (!normalized) return;
  if (state.targets.includes(normalized)) return;
  state.targets.push(normalized);
  saveTargets();
  ensureCard(normalized);
  if (!state.active) {
    setActive(normalized);
  }
}

async function bootstrapDefaultTarget() {
  if (!clusterModeAllowed()) return "";
  try {
    const cfg = await loadBootstrapConfig();
    if (!cfg) return "";
    const defaultAddr = normalizeAddr((cfg.default_orchestrator || "").trim());
    if (!defaultAddr) return "";
    if (!state.targets.includes(defaultAddr)) {
      state.targets.push(defaultAddr);
      saveTargets();
      ensureCard(defaultAddr);
    }
    if (!state.active || !state.targets.includes(state.active)) {
      setActive(defaultAddr);
    }
    return defaultAddr;
  } catch (_) {
    return "";
  }
}

addButton.addEventListener("click", () => {
  addTarget(input.value);
  input.value = "";
});

function setActive(addr) {
  state.active = addr;
  saveActive();
  state.snapshotFailures = 0;
  hideGraphContextMenu();
  resetInstrumentationBuffers();
  // Node IDs are ephemeral in clustered runs; default to Auto when switching target.
  state.activeNodeId = "";
  saveActiveNode();
  state.cards.forEach((card, key) => {
    if (key === addr) {
      card.btn.classList.add("active");
    } else {
      card.btn.classList.remove("active");
    }
  });
  refreshNetworkSelect();
}

function refreshNetworkSelect() {
  const workspace = getActiveWorkspaceMeta();
  if (workspace) {
    const networkId = workspace.network_id || workspace.workspace_id;
    networkSelect.innerHTML = "";
    const opt = document.createElement("option");
    opt.value = networkId;
    opt.textContent = `${workspace.name || workspace.workspace_id} (workspace)`;
    networkSelect.appendChild(opt);
    state.activeNetwork = networkId;
    saveActiveNetwork();
    refreshNodeSelect();
    if (state.activeNetwork && state.activeNetwork !== state.lastNetworkId) {
      state.lastNetworkId = state.activeNetwork;
      setLayoutForActiveNetwork();
    }
    refreshControlButtons();
    syncWorkspaceUi();
    return;
  }

  const networks = state.networksByTarget.get(state.active) || [];
  const current = state.activeNetwork;
  networkSelect.innerHTML = "";
  if (networks.length === 0) {
    const opt = document.createElement("option");
    opt.value = "";
    opt.textContent = "(no networks)";
    networkSelect.appendChild(opt);
    state.activeNetwork = "";
    saveActiveNetwork();
    state.graph = null;
    resetInstrumentationBuffers();
    drawNetwork();
    refreshControlButtons();
    return;
  }
  networks.forEach((n) => {
    const opt = document.createElement("option");
    opt.value = n.network_id;
    opt.textContent = n.network_id;
    networkSelect.appendChild(opt);
  });
  if (!networks.some((n) => n.network_id === current)) {
    state.activeNetwork = networks[0].network_id;
    saveActiveNetwork();
    state.activeNodeId = "";
    saveActiveNode();
  }
  networkSelect.value = state.activeNetwork;
  refreshNodeSelect();
  if (state.activeNetwork && state.activeNetwork !== state.lastNetworkId) {
    state.lastNetworkId = state.activeNetwork;
    setLayoutForActiveNetwork();
  }
  fetchSnapshotForActive();
  refreshControlButtons();
}

networkSelect.addEventListener("change", () => {
  if (isWorkspaceMode()) {
    refreshNetworkSelect();
    return;
  }
  state.activeNetwork = networkSelect.value;
  saveActiveNetwork();
  state.snapshotFailures = 0;
  hideGraphContextMenu();
  resetInstrumentationBuffers();
  state.activeNodeId = "";
  saveActiveNode();
  refreshNodeSelect();
  if (state.activeNetwork && state.activeNetwork !== state.lastNetworkId) {
    state.lastNetworkId = state.activeNetwork;
    setLayoutForActiveNetwork();
  }
  fetchSnapshotForActive();
  refreshControlButtons();
});

function refreshNodeSelect() {
  if (isWorkspaceMode()) {
    nodeSelect.innerHTML = "";
    const workspaceOpt = document.createElement("option");
    workspaceOpt.value = "";
    workspaceOpt.textContent = "Sandbox";
    nodeSelect.appendChild(workspaceOpt);
    state.activeNodeId = "";
    saveActiveNode();
    nodeSelect.value = "";
    return;
  }

  const status = state.statusByTarget.get(state.active);
  const nodes = status ? status.nodes || [] : [];
  nodeSelect.innerHTML = "";
  const autoOpt = document.createElement("option");
  autoOpt.value = "";
  autoOpt.textContent = "Auto";
  nodeSelect.appendChild(autoOpt);
  if (state.activeNetwork) {
    nodes
      .filter((n) => (n.active_networks || []).includes(state.activeNetwork))
      .forEach((n) => {
        const opt = document.createElement("option");
        opt.value = n.node_id;
        opt.textContent = n.node_id;
        nodeSelect.appendChild(opt);
      });
  }
  if (![...nodeSelect.options].some((o) => o.value === state.activeNodeId)) {
    state.activeNodeId = "";
    saveActiveNode();
  }
  nodeSelect.value = state.activeNodeId;
}

nodeSelect.addEventListener("change", () => {
  if (isWorkspaceMode()) {
    refreshNodeSelect();
    return;
  }
  state.activeNodeId = nodeSelect.value;
  saveActiveNode();
  hideGraphContextMenu();
  resetInstrumentationBuffers();
  fetchSnapshotForActive();
});

function renderSidebar(nodes, networks, aggregate = null) {
  const formatGaPacing = (node) =>
    node && node.ga_pacing ? `Yes${node.ga_pacing_reason ? ` (${node.ga_pacing_reason})` : ""}` : "No";
  const formatGaRamp = (node) => {
    if (!node || !node.ga_ramp_active) return "No";
    const pop = Math.max(1, Number(node.ga_ramp_population || 0));
    const workers = Math.max(1, Number(node.ga_ramp_worker_cap || 0));
    const simMs = Number(node.ga_ramp_sim_time_ms || 0);
    return `pop ${pop} | workers ${workers} | sim ${simMs.toFixed(0)} ms`;
  };
  const formatComm = (node) => {
    if (!node || typeof node !== "object") return "unknown";
    const summary = node.comm_protocol || "unknown";
    const peers = node.peer_comm_protocols && typeof node.peer_comm_protocols === "object"
      ? Object.entries(node.peer_comm_protocols)
          .map(([peer, proto]) => `${peer}:${proto}`)
          .sort()
      : [];
    return peers.length ? `${summary} [${peers.join(", ")}]` : summary;
  };

  const dashboardNodes = aggregate?.nodes || nodes;
  const dashboardNetworks = aggregate?.networks || networks;
  const primary =
    nodes.find((n) => state.activeNodeId && n.node_id === state.activeNodeId) ||
    [...nodes].sort((a, b) => Number(b.capacity_score || 0) - Number(a.capacity_score || 0))[0] ||
    null;

  if (!primary) {
    cpuEl.textContent = "0.0%";
    ramEl.textContent = "0 MB";
    tempEl.textContent = "n/a";
    gpuEl.textContent = "Not detected";
    gpuStatusEl.textContent = "Inactive";
    neuronsEl.textContent = "0";
    depthStatusEl.textContent = "0/0";
    capacityScoreEl.textContent = "0.00";
    gaRunningEl.textContent = "No";
    gaPacingEl.textContent = "No";
    gaRampEl.textContent = "No";
    gaProgressEl.textContent = "-";
    gaBestEl.textContent = "-";
    stepTimeEl.textContent = "0.00 ms";
  } else {
    const ramTotal = formatBytes(primary.total_ram);
    const ramAvail = formatBytes(primary.available_ram);
    const gpuCount = Number(primary.num_gpus || 0);
    const neuronCount = Number(primary.num_neurons || 0);
    const redundant = Number(primary.redundant_neurons || 0);
    const curDepth = Number(primary.current_aarnn_depth || 0);
    const wantDepth = Number(primary.desired_aarnn_depth || 0);
    const stepMs = Number(primary.avg_step_time_ms || 0);

    cpuEl.textContent = `${Number(primary.cpu_usage || 0).toFixed(1)}%`;
    ramEl.textContent = `${ramAvail}/${ramTotal}`;
    tempEl.textContent = Number(primary.temperature_c || 0) > 0 ? `${Number(primary.temperature_c).toFixed(1)} C` : "n/a";
    gpuEl.textContent = gpuCount > 0 ? `${gpuCount} detected (OpenCL)` : "Not detected";
    gpuStatusEl.textContent = gpuCount > 0 ? (getActivePlaying() ? "Active" : "Idle") : "Inactive";
    neuronsEl.textContent = redundant > 0 ? `${neuronCount} (+${redundant} redundant)` : `${neuronCount}`;
    depthStatusEl.textContent = `${curDepth}/${wantDepth}`;
    capacityScoreEl.textContent = Number(primary.capacity_score || 0).toFixed(2);
    gaRunningEl.textContent = primary.ga_running ? "Yes" : "No";
    gaPacingEl.textContent = formatGaPacing(primary);
    gaRampEl.textContent = formatGaRamp(primary);
    gaProgressEl.textContent = primary.ga_evaluating
      ? `${Math.round((primary.ga_eval_progress || 0) * 100)}%`
      : primary.ga_running
      ? `Gen ${primary.ga_generation}`
      : "-";
    gaBestEl.textContent =
      typeof primary.ga_best_fitness === "number" ? primary.ga_best_fitness.toFixed(3) : "-";
    stepTimeEl.textContent = `${stepMs.toFixed(2)} ms`;
  }
  activeTargetEl.textContent = state.active || "-";
  nodesCountEl.textContent = dashboardNodes.length.toString();
  networksCountEl.textContent = dashboardNetworks.length.toString();

  const totalClusterEvals = dashboardNodes.reduce((sum, n) => sum + (n.ga_total_evaluations || 0), 0);
  clusterGaEvalsEl.textContent = totalClusterEvals.toString();

  clusterNodesEl.innerHTML = dashboardNodes
    .map((n) => {
      const ramTotal = formatBytes(n.total_ram);
      const ramAvail = formatBytes(n.available_ram);
      const temp = Number(n.temperature_c || 0) > 0 ? `${Number(n.temperature_c).toFixed(1)} C` : "n/a";
      const pacing = n.ga_pacing ? `Pacing: ${n.ga_pacing_reason || "yes"}` : "Pacing: No";
      const ramp = formatGaRamp(n);
      const evals = n.ga_total_evaluations || 0;
      const share = totalClusterEvals > 0 ? ((evals / totalClusterEvals) * 100).toFixed(1) : "0.0";
      const depth = `${Number(n.current_aarnn_depth || 0)}/${Number(n.desired_aarnn_depth || 0)}`;
      const neurons = Number(n.num_neurons || 0);
      const capacity = Number(n.capacity_score || 0).toFixed(2);
      const comm = formatComm(n);
      const nodeLabel = n.node_id || n.address || "node";
      let gaStatus = `GA Evals: ${evals} (${share}%)`;
      if (n.ga_running) {
        const best = typeof n.ga_best_fitness === "number" ? n.ga_best_fitness.toFixed(3) : "-";
        gaStatus += ` | Gen ${n.ga_generation} | Best ${best}`;
      }
      if (ramp !== "No") {
        gaStatus += ` | Ramp ${ramp}`;
      }
      if (n.ga_evaluating) {
        gaStatus += ` | EVALUATING${n.ga_active_eval_seed > 0 ? ` (seed ${n.ga_active_eval_seed})` : ""}`;
      }
      return `<div class="line">${escapeHtml(
        `${nodeLabel} | CPU ${Number(n.cpu_usage || 0).toFixed(1)}% | RAM ${ramAvail}/${ramTotal} | Temp ${temp} | Neurons ${neurons} | Depth ${depth} | Cap ${capacity} | Comm ${comm} | ${pacing}`
      )}<br/><small>${escapeHtml(gaStatus)}</small></div>`;
    })
    .join("");

  clusterNetworksEl.innerHTML = dashboardNetworks
    .map((n) => {
      const stateLabel = n.playing ? "running" : "stopped";
      const distribution = Array.isArray(n.distribution) ? n.distribution : [];
      const distText = distribution
        .map((d) => {
          const counts = Object.entries(d.layer_neuron_counts || {})
            .sort((a, b) => Number(a[0]) - Number(b[0]))
            .map(([layer, count]) => `${layer}(${count})`)
            .join(", ");
          return `${d.node_id}: [${counts}]`;
        })
        .join(" | ");
      return `<div class="line">${escapeHtml(
        `${n.network_id} | ${stateLabel} | dt ${Number(n.current_dt || 0).toFixed(3)} ms | neurons ${Number(n.total_neurons || 0)} | layers ${Number(n.num_layers || 0)}`
      )}${distText ? `<br/><small>${escapeHtml(distText)}</small>` : ""}</div>`;
    })
    .join("");
}

function renderWorkspaceSidebar() {
  const workspace = getActiveWorkspaceMeta();
  const detail = getActiveWorkspaceDetail();
  if (!workspace) {
    setPlaceholder();
    return;
  }

  const status = detail?.status || {};
  const running = Boolean(detail?.summary?.running ?? workspace.running);
  const simTimeMs = Number(detail?.status?.sim_time_ms ?? workspace.sim_time_ms ?? 0);
  const step = Number(detail?.status?.step ?? workspace.step ?? 0);
  const totalNeurons = Number(detail?.status?.total_neurons ?? workspace.total_neurons ?? 0);
  const hiddenLayers = Number(
    detail?.status?.num_hidden_layers ??
      workspace.num_hidden_layers ??
      state.snapshot?.net?.num_hidden_layers ??
      0
  );
  const totalLayers = Math.max(0, hiddenLayers + 1);
  const depth = Number(
    detail?.status?.desired_aarnn_depth ??
      workspace.desired_aarnn_depth ??
      state.snapshot?.net?.aarnn_layer_depth ??
      0
  );
  const updatedAtText =
    Number(workspace.updated_at_ms || 0) > 0
      ? new Date(Number(workspace.updated_at_ms)).toLocaleString()
      : "n/a";

  cpuEl.textContent = "n/a";
  ramEl.textContent = "sandbox";
  tempEl.textContent = "n/a";
  gpuEl.textContent = "Engine-managed";
  gpuStatusEl.textContent = running ? "Active" : "Idle";
  neuronsEl.textContent = `${totalNeurons}`;
  depthStatusEl.textContent = depth > 0 ? `${depth}/${depth}` : "0/0";
  capacityScoreEl.textContent = "n/a";
  gaRunningEl.textContent = "No";
  gaPacingEl.textContent = "No";
  gaRampEl.textContent = "No";
  gaProgressEl.textContent = "-";
  gaBestEl.textContent = "-";
  clusterGaEvalsEl.textContent = "0";
  stepTimeEl.textContent = running ? "engine" : "-";
  activeTargetEl.textContent = `workspace:${workspace.workspace_id}`;
  nodesCountEl.textContent = "1";
  networksCountEl.textContent = "1";
  clusterNodesEl.innerHTML = `<div class="line">${escapeHtml(
    `sandbox | user ${runtimeUserLabel()} | ${running ? "running" : "stopped"} | step ${step} | updated ${updatedAtText}`
  )}</div>`;
  clusterNetworksEl.innerHTML = `<div class="line">${escapeHtml(
    `${workspace.network_id || workspace.workspace_id} | ${running ? "running" : "stopped"} | t ${simTimeMs.toFixed(
      1
    )} ms | neurons ${totalNeurons} | layers ${totalLayers} | model ${
      status.neuron_model || state.lastModel || "aarnn"
    } | learning ${status.learning_rule || state.lastLearning || "aarnn"}`
  )}</div>`;
}

function getActiveNetworkMeta() {
  if (isWorkspaceMode()) {
    return workspaceNetworkMeta(getActiveWorkspaceMeta());
  }
  const networks = state.networksByTarget.get(state.active) || [];
  return networks.find((n) => n.network_id === state.activeNetwork);
}

function playingKey(addr, networkId) {
  if (!addr || !networkId) return "";
  return `${addr}::${networkId}`;
}

function getActivePlaying() {
  if (isWorkspaceMode()) {
    return Boolean(getActiveWorkspaceMeta()?.running);
  }
  const key = playingKey(state.active, state.activeNetwork);
  if (key && state.playingOverride.has(key)) {
    return Boolean(state.playingOverride.get(key));
  }
  const meta = getActiveNetworkMeta();
  return Boolean(meta && meta.playing);
}

function setActiveNetworkPlaying(playing) {
  if (isWorkspaceMode()) {
    const workspace = getActiveWorkspaceMeta();
    const detail = getActiveWorkspaceDetail();
    if (workspace) {
      workspace.running = playing;
    }
    if (detail?.summary) {
      detail.summary.running = playing;
    }
    refreshControlButtons();
    syncWorkspaceUi();
    return;
  }
  const networks = state.networksByTarget.get(state.active);
  if (networks) {
    const meta = networks.find((n) => n.network_id === state.activeNetwork);
    if (meta) {
      meta.playing = playing;
    }
  }
  const key = playingKey(state.active, state.activeNetwork);
  if (key) {
    state.playingOverride.set(key, playing);
  }
  refreshControlButtons();
}

function refreshControlButtons() {
  if (!startStopBtn || !repeatBtn || !resetBtn || !newBtn) return;
  if (isWorkspaceMode()) {
    const workspace = getActiveWorkspaceMeta();
    const playing = getActivePlaying();
    startStopBtn.textContent = playing ? "Stop" : "Start";
    startStopBtn.disabled = !workspace;
    repeatBtn.disabled = !workspace;
    resetBtn.disabled = !workspace;
    newBtn.disabled = !workspace;
    syncWorkspaceUi();
    return;
  }
  const canControl = Boolean(state.active && state.activeNetwork);
  const playing = getActivePlaying();
  startStopBtn.textContent = playing ? "Stop" : "Start";
  startStopBtn.disabled = !canControl;
  repeatBtn.disabled = !canControl;
  resetBtn.disabled = !canControl;
  newBtn.disabled = !canControl;
}

function isAarnnNetwork(meta) {
  return Number(meta?.desired_aarnn_depth || 0) > 0;
}

function setLayout(layout, { save = true, resetView = true } = {}) {
  state.render.layout = layout === "conventional" ? "conventional" : "aarnn";
  if (resetView && state.render.layout === "conventional") {
    state.view.rotation = 0;
  }
  if (save) {
    saveRenderSettings();
  }
  updateLayoutButtons();
  updateNetworkViewLayout();
  rebuildGraph();
}

function setLayoutForActiveNetwork() {
  const meta = getActiveNetworkMeta();
  const desired = isAarnnNetwork(meta) ? "aarnn" : "conventional";
  setLayout(desired, { save: false, resetView: true });
}

function updateLayoutButtons() {
  layoutButtons.forEach((btn) => {
    btn.classList.toggle("active", btn.dataset.layout === state.render.layout);
  });
}

function updateNetworkViewLayout() {
  if (!networkView) return;
  networkView.classList.toggle("conventional", state.render.layout === "conventional");
}

async function pollTarget(addr) {
  try {
    const res = await fetch(`/api/status?addr=${encodeURIComponent(addr)}`);
    if (!res.ok) {
      return null;
    }
    return await res.json();
  } catch (_) {
    return null;
  }
}

async function pollAll() {
  if (state.authMode !== "none" && !state.user) {
    return;
  }
  if (isWorkspaceMode()) {
    renderWorkspaceSidebar();
    refreshNetworkSelect();
    return;
  }
  if (!state.targets.length) {
    state.statusByTarget.clear();
    state.networksByTarget.clear();
    state.active = "";
    renderSidebar([], [], { nodes: [], networks: [] });
    refreshNetworkSelect();
    return;
  }
  const results = await Promise.all(state.targets.map((addr) => pollTarget(addr)));
  results.forEach((result, idx) => {
    const addr = state.targets[idx];
    if (!result) {
      state.networksByTarget.delete(addr);
      state.statusByTarget.delete(addr);
      return;
    }
    const networks = result.networks || [];
    state.networksByTarget.set(addr, networks);
    networks.forEach((n) => {
      const key = playingKey(addr, n.network_id);
      if (key && state.playingOverride.has(key)) {
        if (state.playingOverride.get(key) === Boolean(n.playing)) {
          state.playingOverride.delete(key);
        }
      }
    });
    state.statusByTarget.set(addr, result);
  });

  if (!state.active || !state.targets.includes(state.active)) {
    setActive(state.targets[0]);
  }
  if (statusHealthScore(state.statusByTarget.get(state.active)) === 0) {
    let bestTarget = "";
    let bestScore = 0;
    state.targets.forEach((addr) => {
      const score = statusHealthScore(state.statusByTarget.get(addr));
      if (score > bestScore) {
        bestScore = score;
        bestTarget = addr;
      }
    });
    if (bestTarget && bestTarget !== state.active) {
      setActive(bestTarget);
    }
  }

  const activeStatus = state.statusByTarget.get(state.active);
  const aggregate = aggregateClusterStatus();
  renderSidebar(activeStatus?.nodes || [], activeStatus?.networks || [], aggregate);
  refreshNetworkSelect();
}

async function fetchSnapshotForActive() {
  if (state.authMode !== "none" && !state.user) return;
  const source = activeSource();
  if (!source) return;
  if (snapshotFetchInFlight) {
    snapshotFetchQueued = true;
    return;
  }
  snapshotFetchInFlight = true;
  state.lastSnapshotPollAt = Date.now();
  const requestKey = sourceRequestKey(source);
  let clearGraph = false;
  let url = "";
  let fetcher = fetch;
  if (source.kind === "workspace") {
    url = `/api/runtime/workspaces/${encodeURIComponent(source.workspace.workspace_id)}/snapshot`;
    fetcher = runtimeFetch;
  } else {
    url = `/api/snapshot?addr=${encodeURIComponent(source.addr)}&network_id=${encodeURIComponent(
      source.networkId
    )}`;
    if (source.nodeId) {
      url += `&node_id=${encodeURIComponent(source.nodeId)}`;
    }
  }
  try {
    const res = await fetcher(url);
    if (!res.ok) {
      clearGraph = true;
    } else {
      const data = await res.json();
      if (!data.snapshot_json) {
        clearGraph = true;
      } else {
        const snapshot = JSON.parse(data.snapshot_json);
        const currentKey = sourceRequestKey(activeSource());
        if (requestKey === currentKey) {
          state.snapshotFailures = 0;
          state.snapshot = snapshot;
          syncControlsToSnapshot(snapshot);
          const rebuild = () => {
            const latestKey = sourceRequestKey(activeSource());
            if (latestKey === requestKey) {
              rebuildGraph();
            }
          };
          if (typeof window.requestIdleCallback === "function") {
            window.requestIdleCallback(rebuild, { timeout: 50 });
          } else {
            setTimeout(rebuild, 0);
          }
        } else {
          snapshotFetchQueued = true;
        }
      }
    }
  } catch (_) {
    clearGraph = true;
  } finally {
    if (clearGraph) {
      const currentKey = sourceRequestKey(activeSource());
      if (currentKey === requestKey) {
        state.snapshotFailures = (state.snapshotFailures || 0) + 1;
        // Keep the last rendered graph through brief transport hiccups.
        if (state.snapshotFailures >= 3) {
          state.graph = null;
          state.snapshot = null;
          drawNetwork();
        }
      }
    }
    snapshotFetchInFlight = false;
    if (snapshotFetchQueued) {
      snapshotFetchQueued = false;
      queueMicrotask(() => {
        fetchSnapshotForActive();
      });
    }
  }
}

function snapshotPollIntervalMs() {
  return getActivePlaying() ? SNAPSHOT_POLL_PLAYING_MS : SNAPSHOT_POLL_IDLE_MS;
}

function pollSnapshot() {
  if (state.authMode !== "none" && !state.user) return;
  if (!activeSource()) return;
  const now = Date.now();
  if (now - state.lastSnapshotPollAt < snapshotPollIntervalMs()) return;
  state.lastSnapshotPollAt = now;
  fetchSnapshotForActive();
}

async function pollActivity() {
  if (state.authMode !== "none" && !state.user) return;
  const source = activeSource();
  if (!source) return;
  let url = "";
  let fetcher = fetch;
  if (source.kind === "workspace") {
    url = `/api/runtime/workspaces/${encodeURIComponent(source.workspace.workspace_id)}/activity`;
    fetcher = runtimeFetch;
  } else {
    url = `/api/activity?addr=${encodeURIComponent(source.addr)}&network_id=${encodeURIComponent(
      source.networkId
    )}`;
    if (source.nodeId) {
      url += `&node_id=${encodeURIComponent(source.nodeId)}`;
    }
  }
  try {
    const res = await fetcher(url);
    if (!res.ok) return;
    const data = await res.json();
    const activity =
      source.kind === "workspace"
        ? normalizeActivityPayload(data.activity)
        : normalizeActivityPayload(data);
    state.activity = activity;
    pushInstrumentationFrame(activity);
    drawNetwork();
  } catch (_) {}
}

function buildGraph(snapshot, layout) {
  const net = snapshot.net || {};
  const meta = getActiveNetworkMeta();
  const wIn = snapshot.w_in || { rows: 0, cols: 0, data: [] };
  
  // Use global layer count if available to ensure consistent layout across nodes
  const globalLayers = meta ? meta.num_layers : 0;
  const localHiddenCount = snapshot.w_hh_fwd ? snapshot.w_hh_fwd.length + 1 : 1;
  const hiddenCount = globalLayers > 0 ? (globalLayers - 1) : localHiddenCount;
  
  const hiddenSizes = [];
  if (localHiddenCount > 0) {
    hiddenSizes.push(wIn.rows);
    for (let i = 1; i < localHiddenCount; i += 1) {
      const mat = snapshot.w_hh_fwd[i - 1];
      hiddenSizes.push(mat ? mat.rows : 0);
    }
  }
  // Pad hiddenSizes if local count is less than global
  while (hiddenSizes.length < hiddenCount) {
    hiddenSizes.push(0);
  }

  const sensoryCount = net.num_sensory_neurons || wIn.cols || 0;
  const outputCount = net.num_output_neurons || (snapshot.w_out ? snapshot.w_out.rows : 0);

  const nodes =
    layout === "conventional"
      ? buildConventionalNodes(sensoryCount, hiddenSizes, outputCount)
      : buildAarnnNodes(snapshot, sensoryCount, hiddenSizes, outputCount);

  const edges = [];
  const edgeLimit = state.render.edgeLimit || 6000;
  const weightThreshold = state.render.fullTopology ? 0.0 : (state.render.weightThreshold !== undefined ? state.render.weightThreshold : 0.05);

  if (state.render.fullTopology && snapshot.p_in) {
    addEdgesFromPresence(edges, nodes.sensory, nodes.hidden[0] || [], snapshot.p_in, edgeLimit);
  } else {
    addEdgesFromMatrix(edges, nodes.sensory, nodes.hidden[0] || [], wIn, weightThreshold, edgeLimit);
  }
  if (snapshot.w_hh_fwd) {
    snapshot.w_hh_fwd.forEach((mat, idx) => {
      const presence = snapshot.p_fwd ? snapshot.p_fwd[idx] : null;
      if (state.render.fullTopology && presence) {
        addEdgesFromPresence(edges, nodes.hidden[idx] || [], nodes.hidden[idx + 1] || [], presence, edgeLimit);
      } else {
        addEdgesFromMatrix(edges, nodes.hidden[idx] || [], nodes.hidden[idx + 1] || [], mat, weightThreshold, edgeLimit);
      }
    });
  }
  if (snapshot.w_hh_rec) {
    snapshot.w_hh_rec.forEach((mat, idx) => {
      const presence = snapshot.p_rec ? snapshot.p_rec[idx] : null;
      if (state.render.fullTopology && presence) {
        addEdgesFromPresence(edges, nodes.hidden[idx] || [], nodes.hidden[idx] || [], presence, edgeLimit);
      } else {
        addEdgesFromMatrix(edges, nodes.hidden[idx] || [], nodes.hidden[idx] || [], mat, 0.6, edgeLimit);
      }
    });
  }
  if (snapshot.w_out) {
    if (state.render.fullTopology && snapshot.p_out) {
      addEdgesFromPresence(edges, nodes.hidden[hiddenSizes.length - 1] || [], nodes.output, snapshot.p_out, edgeLimit);
    } else {
      addEdgesFromMatrix(edges, nodes.hidden[hiddenSizes.length - 1] || [], nodes.output, snapshot.w_out, weightThreshold, edgeLimit);
    }
  }

  return { nodes, edges };
}

function buildAarnnNodes(snapshot, sensoryCount, hiddenSizes, outputCount) {
  if (snapshot.topo) {
    return {
      sensory: snapshot.topo.sensory_nodes.map((n, index) => ({
        x: n.x,
        y: n.y,
        kind: "sensory",
        index,
      })),
      output: snapshot.topo.output_nodes.map((n, index) => ({
        x: n.x,
        y: n.y,
        kind: "output",
        index,
      })),
      hidden: snapshot.topo.layers.map((layer, layerIndex) =>
        layer.map((n, index) => ({
          x: n.x,
          y: n.y,
          kind: "hidden",
          layer: layerIndex,
          index,
        }))
      ),
    };
  }
  return {
    sensory: createRingNodes(sensoryCount, 0.65, 0, { kind: "sensory" }),
    hidden: hiddenSizes.map((sz, idx) =>
      createRingNodes(sz, 0.2 + idx * 0.07, 0, { kind: "hidden", layer: idx })
    ),
    output: createRingNodes(outputCount, 0.65, Math.PI / 8, { kind: "output" }),
  };
}

function buildConventionalNodes(sensoryCount, hiddenSizes, outputCount) {
  const totalColumns = hiddenSizes.length + 2;
  const xPositions = [];
  for (let i = 0; i < totalColumns; i += 1) {
    const ratio = totalColumns === 1 ? 0 : i / (totalColumns - 1);
    xPositions.push(-0.9 + ratio * 1.8);
  }
  return {
    sensory: createColumnNodes(sensoryCount, xPositions[0], 0.75, { kind: "sensory" }),
    hidden: hiddenSizes.map((sz, idx) =>
      createColumnNodes(sz, xPositions[idx + 1], 0.75, { kind: "hidden", layer: idx })
    ),
    output: createColumnNodes(outputCount, xPositions[totalColumns - 1], 0.75, {
      kind: "output",
    }),
  };
}

function createRingNodes(count, radius, phase = 0, meta = {}) {
  const nodes = [];
  if (!count) return nodes;
  for (let i = 0; i < count; i += 1) {
    const angle = phase + (i / count) * Math.PI * 2;
    nodes.push({
      x: Math.cos(angle) * radius,
      y: Math.sin(angle) * radius,
      ...meta,
      index: i,
    });
  }
  return nodes;
}

function createColumnNodes(count, x, span, meta = {}) {
  const nodes = [];
  if (!count) return nodes;
  if (count === 1) {
    nodes.push({ x, y: 0, ...meta, index: 0 });
    return nodes;
  }
  for (let i = 0; i < count; i += 1) {
    const t = i / (count - 1);
    nodes.push({ x, y: -span + t * (span * 2), ...meta, index: i });
  }
  return nodes;
}

function addEdgesFromMatrix(edges, fromNodes, toNodes, mat, threshold, limit) {
  if (!mat || !mat.data) return;
  const rows = mat.rows || 0;
  const cols = mat.cols || 0;
  for (let r = 0; r < rows; r += 1) {
    for (let c = 0; c < cols; c += 1) {
      const idx = r * cols + c;
      const w = mat.data[idx];
      if (Math.abs(w) < threshold) continue;
      if (edges.length >= limit) return;
      const from = fromNodes[c];
      const to = toNodes[r];
      if (!from || !to) continue;
      edges.push({ from, to, weight: w });
    }
  }
}

function addEdgesFromPresence(edges, fromNodes, toNodes, mat, limit) {
  if (!mat || !mat.data) return;
  const rows = mat.rows || 0;
  const cols = mat.cols || 0;
  for (let r = 0; r < rows; r += 1) {
    for (let c = 0; c < cols; c += 1) {
      const idx = r * cols + c;
      const w = mat.data[idx];
      if (!w) continue;
      if (edges.length >= limit) return;
      const from = fromNodes[c];
      const to = toNodes[r];
      if (!from || !to) continue;
      edges.push({ from, to, weight: w });
    }
  }
}

function resizeCanvas() {
  const rect = canvas.getBoundingClientRect();
  const ratio = window.devicePixelRatio || 1;
  canvas.width = rect.width * ratio;
  canvas.height = rect.height * ratio;
  ctx.setTransform(ratio, 0, 0, ratio, 0, 0);
  drawNetwork();
}

window.addEventListener("resize", resizeCanvas);

function drawNetwork() {
  const rect = canvas.getBoundingClientRect();
  ctx.clearRect(0, 0, rect.width, rect.height);

  if (!state.graph) {
    edgeCountEl.textContent = "0";
    state.instrumentation.screenNodes = [];
    renderInstrumentation();
    return;
  }
  const { nodes, edges } = state.graph;
  const centerX = rect.width / 2;
  const centerY = rect.height / 2;
  const radius = Math.min(rect.width, rect.height) * 0.32 * state.view.zoom;
  const cosR = Math.cos(state.view.rotation);
  const sinR = Math.sin(state.view.rotation);
  const screenNodes = [];

  // Draw skull membrane (concave hull of hidden nodes) first
  try {
    const allHidden = [];
    nodes.hidden.forEach((layer) => {
      layer.forEach((n) => {
        const r = rotate(n.x, n.y, cosR, sinR);
        const x = centerX + state.view.offsetX + r.x * radius;
        const y = centerY + state.view.offsetY + r.y * radius;
        allHidden.push({x, y});
      });
    });
    if (allHidden.length >= 3) {
      const k = Math.max(3, Math.min(25, Math.floor(Math.sqrt(allHidden.length))));
      const rawHull = concaveHull(allHidden, k);
      if (rawHull && rawHull.length >= 3) {
        const hull = smoothHull(rawHull, 3);
        ctx.beginPath();
        ctx.moveTo(hull[0].x, hull[0].y);
        for (let i = 1; i < hull.length; i += 1) ctx.lineTo(hull[i].x, hull[i].y);
        ctx.closePath();
        ctx.lineWidth = 1.2;
        ctx.strokeStyle = "rgba(200,210,255,0.47)";
        ctx.stroke();
      }
    }
  } catch (e) { /* ignore drawing errors */ }

  ctx.lineWidth = 1;
  ctx.strokeStyle = "rgba(25, 224, 115, 0.35)";

  edges.forEach((edge) => {
    const f = rotate(edge.from.x, edge.from.y, cosR, sinR);
    const t = rotate(edge.to.x, edge.to.y, cosR, sinR);
    const fx = centerX + state.view.offsetX + f.x * radius;
    const fy = centerY + state.view.offsetY + f.y * radius;
    const tx = centerX + state.view.offsetX + t.x * radius;
    const ty = centerY + state.view.offsetY + t.y * radius;
    ctx.beginPath();
    ctx.moveTo(fx, fy);
    ctx.lineTo(tx, ty);
    ctx.stroke();
  });

  const active = state.activity || {};
  const hiddenActive = active.hidden || [];
  const outputActive = active.output ? active.output.indices || [] : [];

  drawNodes(nodes.sensory, centerX, centerY, radius, "#3b6fc4", [], cosR, sinR, screenNodes);
  nodes.hidden.forEach((layer, idx) => {
    const activeIdx = hiddenActive[idx] ? hiddenActive[idx].indices || [] : [];
    drawNodes(layer, centerX, centerY, radius, "#ff9b3c", activeIdx, cosR, sinR, screenNodes);
  });
  drawNodes(nodes.output, centerX, centerY, radius, "#ffd37a", outputActive, cosR, sinR, screenNodes);

  // Draw region labels if enabled
  if (state.render.showRegionLabels && state.snapshot && state.snapshot.net && state.snapshot.net.brain_regions) {
    ctx.font = "12px sans-serif";
    ctx.textAlign = "center";
    state.snapshot.net.brain_regions.forEach((region) => {
      if (region.center) {
        const r = rotate(region.center[0], region.center[1], cosR, sinR);
        const targetX = centerX + state.view.offsetX + r.x * radius;
        const targetY = centerY + state.view.offsetY + r.y * radius;

        const center2DX = centerX + state.view.offsetX;
        const center2DY = centerY + state.view.offsetY;
        let dirX = targetX - center2DX;
        let dirY = targetY - center2DY;
        const dirMag = Math.sqrt(dirX * dirX + dirY * dirY);
        if (dirMag < 1) { dirX = 1; dirY = 0; }
        else { dirX /= dirMag; dirY /= dirMag; }
        const desiredX = targetX + dirX * 30;
        const desiredY = targetY + dirY * 30;

        let stable = state.regionLabelStates.get(region.name);
        if (!stable) {
          stable = { x: desiredX, y: desiredY };
          state.regionLabelStates.set(region.name, stable);
        }

        const smoothing = 0.12;
        stable.x = stable.x * (1 - smoothing) + desiredX * smoothing;
        stable.y = stable.y * (1 - smoothing) + desiredY * smoothing;

        const dx = stable.x - targetX;
        const dy = stable.y - targetY;
        const dist = Math.sqrt(dx * dx + dy * dy);

        if (dist > 5) {
          ctx.beginPath();
          ctx.moveTo(stable.x, stable.y);
          ctx.lineTo(targetX, targetY);
          ctx.strokeStyle = "rgba(255, 255, 255, 0.3)";
          ctx.lineWidth = 1;
          ctx.stroke();
        }

        ctx.fillStyle = "rgba(255, 255, 255, 0.85)";
        ctx.fillText(region.name, stable.x, stable.y);
      }
    });
  }

  edgeCountEl.textContent = edges.length.toString();
  state.instrumentation.screenNodes = screenNodes;
  renderInstrumentation();
}

function drawNodes(nodes, cx, cy, radius, baseColor, activeIndices, cosR, sinR, screenNodes = []) {
  const activeSet = new Set(activeIndices);
  nodes.forEach((node, idx) => {
    const rotated = rotate(node.x, node.y, cosR, sinR);
    const x = cx + state.view.offsetX + rotated.x * radius;
    const y = cy + state.view.offsetY + rotated.y * radius;
    const active = activeSet.has(idx);
    ctx.fillStyle = active ? "#ffffff" : baseColor;
    ctx.beginPath();
    ctx.arc(x, y, active ? 3.4 : 2.2, 0, Math.PI * 2);
    ctx.fill();
    screenNodes.push({
      targetType: node.kind || "sensory",
      layer: Number.isFinite(node.layer) ? node.layer : 0,
      index: Number.isFinite(node.index) ? node.index : idx,
      x,
      y,
    });
  });
}

function rotate(x, y, cosR, sinR) {
  return { x: x * cosR - y * sinR, y: x * sinR + y * cosR };
}

function formatGraphTarget(target) {
  if (!target) return "Node";
  if (target.targetType === "hidden") {
    return `H${target.layer + 1}:${target.index}`;
  }
  if (target.targetType === "output") {
    return `O${target.index}`;
  }
  return `S${target.index}`;
}

function currentSensoryCount() {
  return Number(
    state.snapshot?.net?.num_sensory_neurons || state.graph?.nodes?.sensory?.length || 0
  );
}

function currentOutputCount() {
  return Number(
    state.snapshot?.net?.num_output_neurons || state.graph?.nodes?.output?.length || 0
  );
}

function currentHiddenCount(layer) {
  if (!Number.isFinite(layer) || layer < 0) return 0;
  if (state.graph?.nodes?.hidden?.[layer]) {
    return Number(state.graph.nodes.hidden[layer].length || 0);
  }
  return 0;
}

function probeMatches(probe, target) {
  if (!probe || !target) return false;
  return (
    probe.targetType === target.targetType &&
    Number(probe.layer || 0) === Number(target.layer || 0) &&
    Number(probe.index || 0) === Number(target.index || 0)
  );
}

function findProbeByTarget(target) {
  return state.instrumentation.probes.find((probe) => probeMatches(probe, target)) || null;
}

function setToolStatus(message) {
  if (toolStatusEl) {
    toolStatusEl.textContent = message;
  }
}

function setWorkspaceFeedback(message, tone = "") {
  if (!workspaceFeedbackEl) return;
  workspaceFeedbackEl.textContent = message || "Workspace actions appear here.";
  workspaceFeedbackEl.classList.remove("is-success", "is-error");
  if (tone === "success") workspaceFeedbackEl.classList.add("is-success");
  if (tone === "error") workspaceFeedbackEl.classList.add("is-error");
}

function formatWorkspaceApiError(err, fallback) {
  const fallbackMessage = fallback || "Workspace operation failed.";
  const raw = typeof err?.error === "string" ? err.error.trim() : "";
  const required = Number(err?.required_tokens);
  if (raw) {
    if (Number.isFinite(required) && required > 0 && /insufficient tokens/i.test(raw)) {
      return `${raw}. Token-gated workspace actions are enabled for this deployment.`;
    }
    return raw;
  }
  if (Number.isFinite(required) && required > 0) {
    return `${fallbackMessage} Requires ${required} tokens.`;
  }
  return fallbackMessage;
}

function updateProbeHint() {
  if (!probeHint) return;
  const count = state.instrumentation.probes.length;
  probeHint.textContent = count
    ? `${count} live spike probe${count === 1 ? "" : "s"} active. Right-click a node or use the controls above to add more.`
    : "Right-click a node in the graph to add a spike probe without leaving the canvas.";
}

function syncProbeControls() {
  if (!probeSourceInput || !probeLayerInput || !probeIndexInput) return;
  const targetType = probeSourceInput.value || "sensory";
  const hidden = targetType === "hidden";
  const maxLayer = Math.max(0, (state.graph?.nodes?.hidden?.length || 1) - 1);
  const currentLayer = Math.min(
    maxLayer,
    Math.max(0, Math.trunc(Number(probeLayerInput.value || 0)))
  );

  probeLayerInput.disabled = !hidden;
  probeLayerInput.max = String(maxLayer);
  probeLayerInput.value = String(hidden ? currentLayer : 0);

  let maxIndex = 0;
  if (targetType === "hidden") {
    maxIndex = Math.max(0, currentHiddenCount(currentLayer) - 1);
  } else if (targetType === "output") {
    maxIndex = Math.max(0, currentOutputCount() - 1);
  } else {
    maxIndex = Math.max(0, currentSensoryCount() - 1);
  }
  probeIndexInput.max = String(maxIndex);
  const currentIndex = Math.min(
    maxIndex,
    Math.max(0, Math.trunc(Number(probeIndexInput.value || 0)))
  );
  probeIndexInput.value = String(currentIndex);
  if (addProbeBtn) {
    addProbeBtn.disabled = maxIndex <= 0;
  }
}

function preparePanelCanvas(canvasEl, canvasCtx) {
  if (!canvasEl || !canvasCtx) return null;
  const rect = canvasEl.getBoundingClientRect();
  if (!rect.width || !rect.height) return null;
  const ratio = window.devicePixelRatio || 1;
  const width = Math.max(1, Math.round(rect.width * ratio));
  const height = Math.max(1, Math.round(rect.height * ratio));
  if (canvasEl.width !== width || canvasEl.height !== height) {
    canvasEl.width = width;
    canvasEl.height = height;
  }
  canvasCtx.setTransform(ratio, 0, 0, ratio, 0, 0);
  return { width: rect.width, height: rect.height };
}

function renderEqPanel() {
  if (!eqPanel) return;
  const bands = state.instrumentation.eqBands || [];
  const hasSignal = bands.some((value) => value > 0.03);
  if (eqEmpty) {
    eqEmpty.classList.toggle("hidden", hasSignal);
  }
  eqPanel.innerHTML = bands
    .map((value, index) => {
      const height = Math.max(2, Math.round(value * 88));
      return `<div class="eq-band">
        <div class="eq-band-bar" style="height:${height}px"></div>
        <div class="eq-band-label">B${index + 1}</div>
      </div>`;
    })
    .join("");
}

function drawScopePanel() {
  if (!scopeCanvas || !scopeCtx) return;
  const rect = preparePanelCanvas(scopeCanvas, scopeCtx);
  if (!rect) return;
  scopeCtx.clearRect(0, 0, rect.width, rect.height);
  scopeCtx.fillStyle = "#171717";
  scopeCtx.fillRect(0, 0, rect.width, rect.height);

  const probes = state.instrumentation.probes.filter((probe) => probe.enabled !== false);
  if (probeCountEl) {
    probeCountEl.textContent = String(state.instrumentation.probes.length);
  }
  if (!probes.length) {
    scopeCtx.fillStyle = "#8a8a8a";
    scopeCtx.font = "12px sans-serif";
    scopeCtx.textAlign = "center";
    scopeCtx.fillText("No probes selected", rect.width / 2, rect.height / 2);
    return;
  }

  const left = 12;
  const top = 10;
  const width = rect.width - 24;
  const height = rect.height - 20;
  const laneHeight = height / probes.length;

  for (let lane = 0; lane < probes.length; lane += 1) {
    const probe = probes[lane];
    const laneTop = top + lane * laneHeight;
    const laneBottom = laneTop + laneHeight - 6;
    const laneMid = laneBottom - (laneHeight - 16) * 0.5;

    scopeCtx.strokeStyle = "rgba(255,255,255,0.08)";
    scopeCtx.lineWidth = 1;
    scopeCtx.beginPath();
    scopeCtx.moveTo(left, laneMid);
    scopeCtx.lineTo(left + width, laneMid);
    scopeCtx.stroke();

    scopeCtx.fillStyle = "#8f8f8f";
    scopeCtx.font = "10px sans-serif";
    scopeCtx.textAlign = "left";
    scopeCtx.fillText(formatGraphTarget(probe), left, laneTop + 9);

    const samples = probe.samples || [];
    if (!samples.length) continue;
    scopeCtx.strokeStyle = probe.color;
    scopeCtx.lineWidth = 1.5;
    scopeCtx.beginPath();
    samples.forEach((sample, index) => {
      const x = left + (index / Math.max(1, PROBE_HISTORY - 1)) * width;
      const y =
        laneBottom - 4 - (sample ? laneHeight - 18 : 0);
      if (index === 0) {
        scopeCtx.moveTo(x, y);
      } else {
        scopeCtx.lineTo(x, y);
      }
    });
    scopeCtx.stroke();
  }
}

function drawRasterPanel() {
  if (!rasterCanvas || !rasterCtx) return;
  const rect = preparePanelCanvas(rasterCanvas, rasterCtx);
  if (!rect) return;
  rasterCtx.clearRect(0, 0, rect.width, rect.height);
  rasterCtx.fillStyle = "#171717";
  rasterCtx.fillRect(0, 0, rect.width, rect.height);

  const frames = state.instrumentation.outputRaster || [];
  if (rasterFramesEl) {
    rasterFramesEl.textContent = String(frames.length);
  }
  const outputCount = currentOutputCount();
  if (!frames.length || !outputCount) {
    rasterCtx.fillStyle = "#8a8a8a";
    rasterCtx.font = "12px sans-serif";
    rasterCtx.textAlign = "center";
    rasterCtx.fillText("No output spikes yet", rect.width / 2, rect.height / 2);
    return;
  }

  const left = 8;
  const top = 8;
  const width = rect.width - 16;
  const height = rect.height - 16;
  const cw = width / Math.max(1, frames.length);
  const ch = height / Math.max(1, outputCount);

  rasterCtx.fillStyle = "rgba(255,255,255,0.06)";
  rasterCtx.fillRect(left, top, width, height);

  frames.forEach((frame, columnIndex) => {
    frame.forEach((value, outputIndex) => {
      if (!value) return;
      const x = left + columnIndex * cw;
      const y = top + (outputCount - outputIndex - 1) * ch;
      rasterCtx.fillStyle = "#9ce67a";
      rasterCtx.fillRect(x, y, Math.max(1, cw - 1), Math.max(1, ch - 1));
    });
  });
}

function renderProbeList() {
  if (!scopeProbesEl) return;
  const probes = state.instrumentation.probes;
  if (probeCountEl) {
    probeCountEl.textContent = String(probes.length);
  }
  if (!probes.length) {
    scopeProbesEl.innerHTML = '<div class="muted">No probes selected yet.</div>';
    updateProbeHint();
    return;
  }
  scopeProbesEl.innerHTML = probes
    .map(
      (probe) => `<div class="probe-pill${probe.enabled === false ? " off" : ""}">
        <span class="probe-swatch" style="background:${escapeHtml(probe.color)}"></span>
        <button class="probe-toggle" data-probe-toggle="${probe.id}" type="button">${
          probe.enabled === false ? "Off" : "On"
        }</button>
        <span class="probe-label">${escapeHtml(probe.label)}</span>
        <button class="probe-remove" data-probe-remove="${probe.id}" type="button">×</button>
      </div>`
    )
    .join("");
  scopeProbesEl.querySelectorAll("[data-probe-toggle]").forEach((button) => {
    button.addEventListener("click", () => {
      const probeId = Number(button.getAttribute("data-probe-toggle"));
      const probe = state.instrumentation.probes.find((item) => item.id === probeId);
      if (!probe) return;
      probe.enabled = probe.enabled === false;
      saveInstrumentationState();
      renderInstrumentation();
    });
  });
  scopeProbesEl.querySelectorAll("[data-probe-remove]").forEach((button) => {
    button.addEventListener("click", () => {
      const probeId = Number(button.getAttribute("data-probe-remove"));
      state.instrumentation.probes = state.instrumentation.probes.filter(
        (probe) => probe.id !== probeId
      );
      saveInstrumentationState();
      renderInstrumentation();
      setToolStatus("Removed probe.");
    });
  });
  updateProbeHint();
}

function renderInstrumentation() {
  renderEqPanel();
  drawScopePanel();
  drawRasterPanel();
  renderProbeList();
  syncProbeControls();
}

function addProbe(target) {
  if (!target) return null;
  const maxIndex =
    target.targetType === "hidden"
      ? currentHiddenCount(target.layer)
      : target.targetType === "output"
      ? currentOutputCount()
      : currentSensoryCount();
  if (!maxIndex) {
    setToolStatus("Load an active network snapshot before adding probes.");
    return null;
  }
  if (target.index < 0 || target.index >= maxIndex) {
    setToolStatus(`Probe index ${target.index} is out of range for ${target.targetType}.`);
    return null;
  }
  const existing = findProbeByTarget(target);
  if (existing) {
    existing.enabled = true;
    setToolStatus(`Probe already exists: ${existing.label}`);
    saveInstrumentationState();
    renderInstrumentation();
    return existing;
  }
  const probeId = state.instrumentation.nextProbeId;
  const probe = normalizeProbe(
    {
      id: probeId,
      targetType: target.targetType,
      layer: target.layer,
      index: target.index,
      label: probeDefaultLabel(target.targetType, target.layer || 0, target.index),
      color: PROBE_COLORS[(probeId - 1) % PROBE_COLORS.length],
      enabled: true,
    },
    probeId
  );
  state.instrumentation.nextProbeId += 1;
  state.instrumentation.probes.push(probe);
  saveInstrumentationState();
  renderInstrumentation();
  setToolStatus(`Added probe ${probe.label}.`);
  return probe;
}

function updateEqBands(sensoryIndices) {
  const nextBands = Array.from({ length: EQ_BANDS }, () => 0);
  const sensoryCount = Math.max(
    1,
    currentSensoryCount(),
    sensoryIndices.reduce((maxIndex, rawIndex) => Math.max(maxIndex, Number(rawIndex) + 1), 0)
  );
  if (sensoryIndices.length) {
    sensoryIndices.forEach((rawIndex) => {
      const index = Number(rawIndex);
      if (!Number.isFinite(index) || index < 0) return;
      const band = Math.max(
        0,
        Math.min(EQ_BANDS - 1, Math.floor((index / sensoryCount) * EQ_BANDS))
      );
      nextBands[band] += 1;
    });
    const denom = Math.max(1, sensoryIndices.length);
    for (let i = 0; i < nextBands.length; i += 1) {
      nextBands[i] = Math.min(1, nextBands[i] / denom);
    }
  }
  state.instrumentation.eqBands = state.instrumentation.eqBands.map((previous, index) => {
    const target = nextBands[index] || 0;
    if (!sensoryIndices.length) {
      return previous * 0.95;
    }
    return previous * 0.72 + target * 0.28;
  });
}

function pushOutputRasterFrame(outputIndices) {
  const outputCount = currentOutputCount();
  if (!outputCount) return;
  const frame = Array.from({ length: outputCount }, () => 0);
  outputIndices.forEach((rawIndex) => {
    const index = Number(rawIndex);
    if (Number.isFinite(index) && index >= 0 && index < outputCount) {
      frame[index] = 1;
    }
  });
  state.instrumentation.outputRaster.push(frame);
  while (state.instrumentation.outputRaster.length > RASTER_HISTORY) {
    state.instrumentation.outputRaster.shift();
  }
}

function readProbeValue(probe, sensorySet, hiddenSets, outputSet) {
  if (probe.targetType === "hidden") {
    return hiddenSets[probe.layer] && hiddenSets[probe.layer].has(probe.index) ? 1 : 0;
  }
  if (probe.targetType === "output") {
    return outputSet.has(probe.index) ? 1 : 0;
  }
  return sensorySet.has(probe.index) ? 1 : 0;
}

function pushProbeSamples(activity) {
  const sensorySet = new Set((activity?.sensory?.indices || []).map((index) => Number(index)));
  const hiddenSets = Array.isArray(activity?.hidden)
    ? activity.hidden.map((layer) => new Set((layer?.indices || []).map((index) => Number(index))))
    : [];
  const outputSet = new Set((activity?.output?.indices || []).map((index) => Number(index)));
  state.instrumentation.probes.forEach((probe) => {
    probe.samples.push(readProbeValue(probe, sensorySet, hiddenSets, outputSet));
    while (probe.samples.length > PROBE_HISTORY) {
      probe.samples.shift();
    }
  });
}

function pushInstrumentationFrame(activity) {
  const sensoryIndices = activity?.sensory?.indices || [];
  const outputIndices = activity?.output?.indices || [];
  updateEqBands(sensoryIndices);
  pushOutputRasterFrame(outputIndices);
  pushProbeSamples(activity || {});
  renderInstrumentation();
}

function findNearestGraphNode(clientX, clientY) {
  if (!canvas || !state.instrumentation.screenNodes.length) return null;
  const rect = canvas.getBoundingClientRect();
  const x = clientX - rect.left;
  const y = clientY - rect.top;
  let best = null;
  let bestDist = Infinity;
  state.instrumentation.screenNodes.forEach((node) => {
    const dx = node.x - x;
    const dy = node.y - y;
    const dist = Math.sqrt(dx * dx + dy * dy);
    if (dist < bestDist) {
      best = node;
      bestDist = dist;
    }
  });
  return bestDist <= 14 ? best : null;
}

function hideGraphContextMenu() {
  if (graphContextMenu) {
    graphContextMenu.classList.add("hidden");
  }
  state.instrumentation.contextTarget = null;
}

function showGraphContextMenu(target, clientX, clientY) {
  if (!graphContextMenu || !graphContextTitle || !graphContextDetails || !graphAddProbeBtn) return;
  state.instrumentation.contextTarget = target;
  const existing = findProbeByTarget(target);
  graphContextTitle.textContent = formatGraphTarget(target);
  graphContextDetails.textContent =
    target.targetType === "hidden"
      ? `Hidden layer ${target.layer + 1}, neuron ${target.index}.`
      : target.targetType === "output"
      ? `Output neuron ${target.index}.`
      : `Sensory neuron ${target.index}.`;
  graphAddProbeBtn.textContent = existing ? "Remove Probe" : "Add Probe";
  graphContextMenu.style.left = `${Math.max(8, Math.min(window.innerWidth - 240, clientX + 12))}px`;
  graphContextMenu.style.top = `${Math.max(8, Math.min(window.innerHeight - 140, clientY + 12))}px`;
  graphContextMenu.classList.remove("hidden");
}

function downloadTextFile(filename, text) {
  const blob = new Blob([text], { type: "application/json;charset=utf-8" });
  const url = URL.createObjectURL(blob);
  const link = document.createElement("a");
  link.href = url;
  link.download = filename;
  document.body.appendChild(link);
  link.click();
  link.remove();
  URL.revokeObjectURL(url);
}

function pickJsonFile() {
  return new Promise((resolve) => {
    const input = document.createElement("input");
    input.type = "file";
    input.accept = ".json,application/json";
    input.addEventListener("change", async () => {
      const file = input.files && input.files[0];
      if (!file) {
        resolve(null);
        return;
      }
      try {
        const text = await file.text();
        resolve(text);
      } catch (_) {
        resolve(null);
      }
    });
    input.click();
  });
}

async function applyRemoteJsonPayload(raw, label) {
  const payloadKind = label.toLowerCase().includes("snapshot") ? "snapshot" : "config";
  if (isWorkspaceMode()) {
    const workspace = getActiveWorkspaceMeta();
    if (!workspace) {
      setToolStatus(`Select a workspace before loading ${label.toLowerCase()}.`);
      return false;
    }
    const ok = await importWorkspacePayload(raw, payloadKind, {
      replaceBaseline: true,
    });
    if (ok) {
      setToolStatus(`${label} applied to workspace ${workspace.workspace_id}.`);
      resetInstrumentationBuffers();
    }
    return ok;
  }
  if (!state.active || !state.activeNetwork) {
    setToolStatus(`Select an active target and network before loading ${label.toLowerCase()}.`);
    return false;
  }
  try {
    const res = await fetch("/api/update_network", {
      method: "POST",
      headers: { "Content-Type": "application/json" },
      body: JSON.stringify({
        addr: state.active,
        network_id: state.activeNetwork,
        config_json: raw,
      }),
    });
    if (!res.ok) {
      const err = await res.json().catch(() => ({}));
      setToolStatus(err.error || `Failed to load ${label.toLowerCase()}.`);
      return false;
    }
    setToolStatus(`${label} applied to ${state.activeNetwork}.`);
    resetInstrumentationBuffers();
    await pollAll();
    await fetchSnapshotForActive();
    return true;
  } catch (_) {
    setToolStatus(`Failed to load ${label.toLowerCase()}.`);
    return false;
  }
}

function currentConfigJson() {
  if (state.snapshot?.net) {
    return JSON.stringify(state.snapshot.net, null, 2);
  }
  const meta = getActiveNetworkMeta();
  if (meta?.config_json) {
    try {
      return JSON.stringify(JSON.parse(meta.config_json), null, 2);
    } catch (_) {
      return meta.config_json;
    }
  }
  return "";
}

function currentNetworkJson() {
  return state.snapshot ? JSON.stringify(state.snapshot, null, 2) : "";
}

function normalizeIndicesEnvelope(raw) {
  if (Array.isArray(raw)) {
    return {
      indices: raw.map((index) => Number(index)).filter((index) => Number.isFinite(index) && index >= 0),
    };
  }
  if (raw && Array.isArray(raw.indices)) {
    return {
      indices: raw.indices
        .map((index) => Number(index))
        .filter((index) => Number.isFinite(index) && index >= 0),
    };
  }
  return { indices: [] };
}

function normalizeActivityPayload(activity) {
  if (!activity || typeof activity !== "object") {
    return {
      sensory: { indices: [] },
      hidden: [],
      output: { indices: [] },
    };
  }
  return {
    ...activity,
    sensory: normalizeIndicesEnvelope(activity.sensory),
    hidden: Array.isArray(activity.hidden)
      ? activity.hidden.map((layer) => normalizeIndicesEnvelope(layer))
      : [],
    output: normalizeIndicesEnvelope(activity.output),
  };
}

function smoothHull(points, iterations = 2) {
  if (!points || points.length < 3) return points || [];
  let current = points;
  for (let iter = 0; iter < iterations; iter++) {
    const next = [];
    for (let i = 0; i < current.length; i++) {
      const p1 = current[i];
      const p2 = current[(i + 1) % current.length];
      next.push({
        x: 0.75 * p1.x + 0.25 * p2.x,
        y: 0.75 * p1.y + 0.25 * p2.y
      });
      next.push({
        x: 0.25 * p1.x + 0.75 * p2.x,
        y: 0.25 * p1.y + 0.75 * p2.y
      });
    }
    current = next;
  }
  return current;
}

// k-NN Concave Hull (Moreira & Santos, simplified)
function concaveHull(points, k) {
  if (!points || points.length < 4) return points || [];
  // Copy
  const pts = points.slice().sort((a,b)=> a.x===b.x ? a.y-b.y : a.x-b.x);
  const start = { x: pts[0].x, y: pts[0].y };
  const hull = [start];
  let current = start;
  let prevAngle = 0.0; // radians, pointing to +x
  // Remove start from candidates
  const remaining = pts.slice(1);

  function dist2(a,b){ const dx=a.x-b.x, dy=a.y-b.y; return dx*dx+dy*dy; }
  function ang(a,b){ return Math.atan2(b.y-a.y, b.x-a.x); }

  let guard = 0;
  while (remaining.length && guard++ < 10000) {
    remaining.sort((p,q) => dist2(current,p) - dist2(current,q));
    const neigh = remaining.slice(0, Math.min(k, remaining.length));
    let best = null;
    let bestScore = Infinity;
    for (const p of neigh) {
      const a = ang(current, p);
      let turn = a - prevAngle;
      while (turn <= -Math.PI) turn += 2*Math.PI;
      while (turn > Math.PI) turn -= 2*Math.PI;
      const score = turn < 0 ? turn + 2*Math.PI : turn;
      if (score < bestScore) { bestScore = score; best = p; }
    }
    if (!best) break;
    hull.push(best);
    prevAngle = ang(current, best);
    current = best;
    const idx = remaining.indexOf(best);
    if (idx >= 0) remaining.splice(idx,1);
    if (Math.abs(current.x - start.x) < 1.0 && Math.abs(current.y - start.y) < 1.0 && hull.length > 3) break;
  }
  return hull;
}

function setPlaceholder() {
  cpuEl.textContent = "0.0%";
  ramEl.textContent = "0 MB";
  tempEl.textContent = "n/a";
  gpuEl.textContent = "Not detected";
  gpuStatusEl.textContent = "Inactive";
  neuronsEl.textContent = "0";
  depthStatusEl.textContent = "0/0";
  capacityScoreEl.textContent = "0.00";
  gaRunningEl.textContent = "No";
  gaPacingEl.textContent = "No";
  gaRampEl.textContent = "No";
  gaProgressEl.textContent = "-";
  gaBestEl.textContent = "-";
  clusterGaEvalsEl.textContent = "0";
  stepTimeEl.textContent = "0.00 ms";
  activeTargetEl.textContent = "-";
  nodesCountEl.textContent = "0";
  networksCountEl.textContent = "0";
  clusterNodesEl.innerHTML = "";
  clusterNetworksEl.innerHTML = "";
  resetInstrumentationBuffers();
  renderInstrumentation();
}

function rebuildGraph() {
  if (!state.snapshot) {
    state.graph = null;
    drawNetwork();
    return;
  }
  state.graph = buildGraph(state.snapshot, state.render.layout);
  drawNetwork();
}

function syncControlsToSnapshot(snapshot) {
  if (!snapshot || !snapshot.net) return;
  const net = snapshot.net;
  
  const meta = getActiveNetworkMeta();
  if (meta) {
    if (meta.neuron_model && meta.neuron_model !== state.lastModel) {
      state.lastModel = meta.neuron_model;
      updateSegmentedSelector(modelSelector, meta.neuron_model);
    }
    if (meta.learning_rule && meta.learning_rule !== state.lastLearning) {
      state.lastLearning = meta.learning_rule;
      updateSegmentedSelector(learningSelector, meta.learning_rule);
    }
  }

  aarnnRandomness.value = net.aarnn_synaptic_energy_randomness;
  aarnnRandomnessValue.textContent = net.aarnn_synaptic_energy_randomness.toFixed(2);
  const depth = (typeof net.aarnn_layer_depth === 'number') ? net.aarnn_layer_depth : 5;
  aarnnDepth.value = depth;
  aarnnDepthValue.textContent = depth;
  useDelays.checked = net.use_aarnn_delays;
  useMorphology.checked = net.use_morphology;
  useStp.checked = net.aarnn_bio?.stp_enabled ?? true;
  useNeuromod.checked = net.aarnn_bio?.neuromodulation_enabled ?? true;
  evolution3d.checked = net.growth_enabled;
  growth3dInput.checked = net.growth_enabled; // Assuming they are linked for now
  clumpingDesign.value = net.clumping_design || "HumanBrain";
}

function updateSegmentedSelector(selector, value) {
  if (!selector) return;
  const buttons = selector.querySelectorAll("button");
  buttons.forEach(btn => {
    const btnValue = btn.dataset.model || btn.dataset.learning;
    if (btnValue === value) {
      btn.classList.add("active");
    } else {
      btn.classList.remove("active");
    }
  });
}

function buildAarnnHumanDefaults() {
  return {
    growth_enabled: true,
    use_morphology: true,
    morpho_growth_enabled: true,
    use_aarnn_delays: true,
    aarnn_layer_depth: 5,
    clumping_design: "HumanBrain",

    aarnn_velocity: 10.0,
    axon_velocity: 20.0,
    dend_velocity: 5.0,
    p_release_default: 0.7,
    bouton_latency_ms: 0.5,
    bouton_jitter_ms: 0.1,

    enforce_unique_geometry: true,
    use_mid_bends: true,

    aarnn_synaptic_energy_randomness: 0.1,
    aarnn_resonance_gain: 0.2,
    aarnn_resonance_decay: 0.1,
    aarnn_neuromod_baseline_dopamine: 1.0,
    aarnn_neuromod_baseline_ach: 1.0,
    aarnn_neuromod_baseline_serotonin: 1.0,
    aarnn_neuromod_dopamine_signal: "perceptual_error",
    aarnn_neuromod_ach_signal: "sensory_spikes",
    aarnn_neuromod_serotonin_signal: "stability",
    aarnn_reward_proxy: 0.0,
    aarnn_neuromod_decay: 0.05,
    aarnn_neuromod_error_gain: 0.0,
    aarnn_neuromod_activity_gain: 0.0,
    aarnn_neuromod_stability_gain: 0.0,

    aarnn_inhibitory_fraction: 0.2,
    aarnn_dale_strictness: 0.75,
    aarnn_gap_junction_strength: 0.02,
    aarnn_nmda_voltage_sensitivity: 0.04,
    aarnn_triplet_ltp_gain: 0.25,
    aarnn_triplet_ltd_gain: 0.15,
    aarnn_synaptic_scaling_strength: 0.02,
    aarnn_synaptic_scaling_target: 1.0,
    aarnn_distance_attenuation_per_unit: 0.15,
    aarnn_release_prob_heterogeneity: 0.1,

    aarnn_bio: {
      stp_enabled: true,
      neuromodulation_enabled: true,
    },
  };
}

async function updateNetworkSettings(options = {}) {
  if (!activeSource()) return;
  const forceBaseline = options.forceBaseline === true;
  
  const activeModel = modelSelector.querySelector("button.active")?.dataset.model;
  const activeLearning = learningSelector.querySelector("button.active")?.dataset.learning;
  
  // Clone current config if possible, or start with AARNN human-brain defaults.
  const config = (!forceBaseline && state.snapshot?.net)
    ? { ...state.snapshot.net }
    : buildAarnnHumanDefaults();
  
  config.aarnn_synaptic_energy_randomness = parseFloat(aarnnRandomness.value);
  config.aarnn_layer_depth = parseInt(aarnnDepth.value);
  config.use_aarnn_delays = useDelays.checked;
  config.use_morphology = useMorphology.checked;
  if (!config.aarnn_bio) config.aarnn_bio = {};
  config.aarnn_bio.stp_enabled = useStp.checked;
  config.aarnn_bio.neuromodulation_enabled = useNeuromod.checked;
  config.growth_enabled = evolution3d.checked;
  config.clumping_design = clumpingDesign.value;
  const configJson = JSON.stringify(config);

  if (isWorkspaceMode()) {
    await importWorkspacePayload(configJson, "config", {
      replaceBaseline: true,
      neuron_model: activeModel,
      learning_rule: activeLearning,
    });
    return;
  }

  const payload = {
    addr: state.active,
    network_id: state.activeNetwork,
    config_json: configJson,
    neuron_model: activeModel,
    learning_rule: activeLearning
  };
  
  try {
    const res = await fetch("/api/update_network", {
      method: "POST",
      headers: { "Content-Type": "application/json" },
      body: JSON.stringify(payload)
    });
    if (res.ok) {
      console.log("Network settings updated successfully");
    } else {
      console.error("Failed to update network settings");
    }
  } catch (e) {
    console.error("Error updating network settings:", e);
  }
}

async function sendControlAction(action) {
  if (isWorkspaceMode()) {
    const ok = await controlWorkspaceAction(action);
    if (ok) {
      if (action === "start" || action === "repeat") {
        setActiveNetworkPlaying(true);
      } else if (action === "stop" || action === "reset" || action === "new") {
        setActiveNetworkPlaying(false);
      }
    }
    return;
  }
  if (!state.active || !state.activeNetwork) return;
  const payload = {
    addr: state.active,
    network_id: state.activeNetwork,
    action,
  };
  try {
    const res = await fetch("/api/control_network", {
      method: "POST",
      headers: { "Content-Type": "application/json" },
      body: JSON.stringify(payload),
    });
    if (res.ok) {
      if (action === "start" || action === "repeat") {
        setActiveNetworkPlaying(true);
      } else if (action === "stop" || action === "reset" || action === "new") {
        setActiveNetworkPlaying(false);
      }
      await pollAll();
      fetchSnapshotForActive();
    } else {
      console.error("Failed to send control action");
    }
  } catch (e) {
    console.error("Error sending control action:", e);
  }
}

async function initTargets() {
  if (!clusterModeAllowed()) {
    resetTargetsUi();
    state.statusByTarget.clear();
    state.networksByTarget.clear();
    state.active = "";
    state.activeNetwork = "";
    state.activeNodeId = "";
    renderWorkspaceSidebar();
    refreshNetworkSelect();
    return;
  }
  const defaultAddr = await bootstrapDefaultTarget();
  if (state.targets.length === 0) {
    if (isWorkspaceMode()) {
      renderWorkspaceSidebar();
      refreshNetworkSelect();
    } else {
      setPlaceholder();
    }
    return;
  }
  state.targets.forEach((addr) => ensureCard(addr));
  if (!state.active || !state.targets.includes(state.active)) {
    setActive(defaultAddr || state.targets[0]);
  } else {
    setActive(state.active);
  }
  await pollAll();
  const activeScore = statusHealthScore(state.statusByTarget.get(state.active));
  const defaultScore = statusHealthScore(state.statusByTarget.get(defaultAddr));
  if (defaultAddr && state.active !== defaultAddr && defaultScore > activeScore) {
    setActive(defaultAddr);
    await pollAll();
    return;
  }
  if (activeScore === 0) {
    let bestTarget = "";
    let bestScore = 0;
    state.targets.forEach((addr) => {
      const score = statusHealthScore(state.statusByTarget.get(addr));
      if (score > bestScore) {
        bestScore = score;
        bestTarget = addr;
      }
    });
    if (bestTarget && bestTarget !== state.active) {
      setActive(bestTarget);
      await pollAll();
    }
  }
}

function formatBytes(bytes) {
  if (!bytes) return "0";
  const units = ["B", "KB", "MB", "GB", "TB"];
  let value = bytes;
  let idx = 0;
  while (value >= 1024 && idx < units.length - 1) {
    value /= 1024;
    idx += 1;
  }
  return `${value.toFixed(1)}${units[idx]}`;
}

function escapeHtml(str) {
  return str.replace(/[&<>"']/g, (c) => {
    switch (c) {
      case "&":
        return "&amp;";
      case "<":
        return "&lt;";
      case ">":
        return "&gt;";
      case '"':
        return "&quot;";
      case "'":
        return "&#39;";
      default:
        return c;
    }
  });
}

function syncRenderControls() {
  fullTopologyToggle.checked = state.render.fullTopology;
  edgeLimitInput.value = state.render.edgeLimit;
  edgeLimitValue.textContent = state.render.edgeLimit.toString();
  weightThresholdInput.value = state.render.weightThreshold.toFixed(2);
  weightThresholdValue.textContent = state.render.weightThreshold.toFixed(2);
  updateLayoutButtons();
  updateNetworkViewLayout();
  showRegionLabelsInput.checked = state.render.showRegionLabels;
}

function setIoStatus(text, cssClass = "io-status-idle") {
  state.io.status = text;
  state.io.statusClass = cssClass;
  if (!ioSourceStatus) return;
  ioSourceStatus.textContent = text;
  ioSourceStatus.classList.remove(
    "io-status-idle",
    "io-status-connecting",
    "io-status-active",
    "io-status-error"
  );
  ioSourceStatus.classList.add(cssClass);
}

function syncIoControls() {
  if (!ioInputSource || !ioInputUrl || !ioAerBase || !ioSourceToggle) return;
  ioInputSource.value = state.io.sourceType || "none";
  ioInputUrl.value = state.io.sourceUrl || "";
  ioAerBase.value = Number.isFinite(Number(state.io.aerBase)) ? Number(state.io.aerBase) : 0;

  const sourceEnabled = ioInputSource.value === "aer-http-stream";
  ioInputUrl.disabled = !sourceEnabled || state.io.streaming;
  ioAerBase.disabled = !sourceEnabled || state.io.streaming;
  ioSourceToggle.disabled = !sourceEnabled;
  ioSourceToggle.textContent = state.io.streaming ? "Disconnect" : "Connect";

  if (!state.io.status) {
    setIoStatus("Disconnected", "io-status-idle");
  } else if (ioSourceStatus) {
    setIoStatus(state.io.status, state.io.statusClass || "io-status-idle");
  }
}

function normalizeAerStreamFrame(line) {
  const trimmed = line.trim();
  if (!trimmed) return null;
  if (trimmed.startsWith("{")) {
    return JSON.parse(trimmed);
  }
  return { aer_payload_hex: trimmed };
}

async function sendAerFrameToApi(frame, ctxDefaults) {
  const payload = {
    addr: ctxDefaults.addr,
    network_id:
      typeof frame.network_id === "string" && frame.network_id
        ? frame.network_id
        : ctxDefaults.networkId,
    aer_base:
      frame.aer_base !== undefined && frame.aer_base !== null
        ? Number(frame.aer_base)
        : Number(state.io.aerBase || 0),
  };

  if (typeof frame.node_id === "string" && frame.node_id) {
    payload.node_id = frame.node_id;
  }
  if (typeof frame.step_index === "number" && Number.isFinite(frame.step_index)) {
    payload.step_index = Math.trunc(frame.step_index);
  }
  if (typeof frame.is_backward === "boolean") {
    payload.is_backward = frame.is_backward;
  }
  if (typeof frame.aer_payload_hex === "string" && frame.aer_payload_hex.trim()) {
    payload.aer_payload_hex = frame.aer_payload_hex.trim();
  }
  if (Array.isArray(frame.spike_indices)) {
    payload.spike_indices = frame.spike_indices
      .map((v) => Number(v))
      .filter((v) => Number.isFinite(v) && v >= 0)
      .map((v) => Math.trunc(v));
  }
  if (!payload.aer_payload_hex && (!payload.spike_indices || payload.spike_indices.length === 0)) {
    return;
  }

  const resp = await fetch("/api/aer/inject", {
    method: "POST",
    headers: { "Content-Type": "application/json" },
    body: JSON.stringify(payload),
  });
  if (!resp.ok) {
    let message = `AER inject failed (${resp.status})`;
    try {
      const err = await resp.json();
      if (err && err.error) {
        message = err.error;
      }
    } catch (_) {}
    throw new Error(message);
  }
}

async function startIoSourceStream() {
  if (state.io.streaming) return;
  if (state.io.sourceType !== "aer-http-stream") {
    setIoStatus("Source disabled", "io-status-idle");
    return;
  }
  if (!state.io.sourceUrl) {
    setIoStatus("Enter a source URL", "io-status-error");
    return;
  }
  if (!/^https?:\/\//i.test(state.io.sourceUrl)) {
    setIoStatus("Source URL must start with http:// or https://", "io-status-error");
    return;
  }
  if (!state.active || !state.activeNetwork) {
    setIoStatus("Select active target + network first", "io-status-error");
    return;
  }

  const controller = new AbortController();
  const defaults = {
    addr: state.active,
    networkId: state.activeNetwork,
  };
  state.io.streaming = true;
  state.io.connectedAt = Date.now();
  state.io.defaultAddr = defaults.addr;
  state.io.defaultNetworkId = defaults.networkId;
  ioSourceRunner = { controller, frames: 0 };
  syncIoControls();
  setIoStatus("Connecting...", "io-status-connecting");

  try {
    const resp = await fetch(state.io.sourceUrl, {
      method: "GET",
      cache: "no-store",
      signal: controller.signal,
    });
    if (!resp.ok) {
      throw new Error(`Source request failed (${resp.status})`);
    }
    if (!resp.body) {
      throw new Error("Source returned no stream body");
    }

    setIoStatus(
      `Streaming -> ${defaults.networkId}@${defaults.addr}`,
      "io-status-active"
    );
    const reader = resp.body.getReader();
    const decoder = new TextDecoder();
    let buffer = "";
    while (state.io.streaming) {
      const { value, done } = await reader.read();
      if (done) break;
      buffer += decoder.decode(value, { stream: true });
      const lines = buffer.split(/\r?\n/);
      buffer = lines.pop() || "";
      for (const line of lines) {
        const trimmed = line.trim();
        if (!trimmed) continue;
        const frame = normalizeAerStreamFrame(trimmed);
        if (!frame) continue;
        await sendAerFrameToApi(frame, defaults);
        if (!ioSourceRunner) break;
        ioSourceRunner.frames += 1;
        if (ioSourceRunner.frames % 20 === 0) {
          setIoStatus(
            `Streaming ${ioSourceRunner.frames} frames -> ${defaults.networkId}@${defaults.addr}`,
            "io-status-active"
          );
        }
      }
    }

    if (buffer.trim()) {
      const frame = normalizeAerStreamFrame(buffer.trim());
      if (frame) {
        await sendAerFrameToApi(frame, defaults);
        if (ioSourceRunner) {
          ioSourceRunner.frames += 1;
        }
      }
    }

    if (!controller.signal.aborted) {
      const total = ioSourceRunner ? ioSourceRunner.frames : 0;
      setIoStatus(
        `Disconnected (source closed, ${total} frames)`,
        "io-status-idle"
      );
    }
  } catch (error) {
    if (controller.signal.aborted) {
      setIoStatus("Disconnected", "io-status-idle");
    } else {
      setIoStatus(
        `Source error: ${error instanceof Error ? error.message : String(error)}`,
        "io-status-error"
      );
    }
  } finally {
    state.io.streaming = false;
    ioSourceRunner = null;
    syncIoControls();
  }
}

function stopIoSourceStream() {
  state.io.streaming = false;
  if (ioSourceRunner && ioSourceRunner.controller) {
    ioSourceRunner.controller.abort();
  }
  ioSourceRunner = null;
  setIoStatus("Disconnected", "io-status-idle");
  syncIoControls();
}

function attachIoControls() {
  if (!ioInputSource || !ioInputUrl || !ioAerBase || !ioSourceToggle) return;
  syncIoControls();

  ioInputSource.addEventListener("change", () => {
    state.io.sourceType = ioInputSource.value === "aer-http-stream" ? "aer-http-stream" : "none";
    if (state.io.sourceType === "none" && state.io.streaming) {
      stopIoSourceStream();
    } else {
      syncIoControls();
    }
    saveIoSettings();
  });

  ioInputUrl.addEventListener("change", () => {
    state.io.sourceUrl = ioInputUrl.value.trim();
    saveIoSettings();
  });
  ioInputUrl.addEventListener("blur", () => {
    state.io.sourceUrl = ioInputUrl.value.trim();
    saveIoSettings();
  });

  ioAerBase.addEventListener("change", () => {
    const v = Number(ioAerBase.value);
    state.io.aerBase = Number.isFinite(v) && v >= 0 ? Math.trunc(v) : 0;
    ioAerBase.value = state.io.aerBase;
    saveIoSettings();
  });

  ioSourceToggle.addEventListener("click", () => {
    if (state.io.streaming) {
      stopIoSourceStream();
    } else {
      startIoSourceStream();
    }
  });
}

window.addEventListener("beforeunload", () => {
  if (state.io.streaming) {
    stopIoSourceStream();
  }
});

function attachControls() {
  syncRenderControls();
  attachIoControls();
  renderInstrumentation();
  syncWorkspaceUi();
  if (workspaceSelect) {
    workspaceSelect.addEventListener("change", async () => {
      state.runtime.activeWorkspace = workspaceSelect.value || "";
      saveActiveWorkspace();
      state.snapshotFailures = 0;
      hideGraphContextMenu();
      resetInstrumentationBuffers();
      state.activity = null;
      refreshNetworkSelect();
      if (state.runtime.activeWorkspace) {
        await loadWorkspaceDetail(state.runtime.activeWorkspace);
        await fetchSnapshotForActive();
        await pollActivity();
        renderWorkspaceSidebar();
      } else {
        await pollAll();
        if (state.active && state.activeNetwork) {
          await fetchSnapshotForActive();
          await pollActivity();
        }
      }
      refreshControlButtons();
    });
  }
  if (workspaceUserInput) {
    workspaceUserInput.addEventListener("change", async () => {
      if (state.authMode !== "none") {
        syncWorkspaceUi();
        return;
      }
      const nextUser = workspaceUserInput.value.trim() || defaultRuntimeUser();
      state.runtime.userId = nextUser;
      saveRuntimeUser();
      state.runtime.workspaces = [];
      state.runtime.details.clear();
      await loadRuntimeStatus();
      refreshNetworkSelect();
      if (isWorkspaceMode()) {
        await fetchSnapshotForActive();
        await pollActivity();
      } else {
        await pollAll();
      }
      setToolStatus(`Runtime sandbox user set to ${nextUser}.`);
    });
  }
  if (workspaceRefreshBtn) {
    workspaceRefreshBtn.addEventListener("click", async () => {
      await loadRuntimeStatus();
      if (isWorkspaceMode()) {
        await fetchSnapshotForActive();
        await pollActivity();
      } else {
        await pollAll();
      }
    });
  }
  if (workspaceCreateBtn) {
    workspaceCreateBtn.addEventListener("click", () => {
      createWorkspaceFromCurrentState();
    });
  }
  if (workspaceDeleteBtn) {
    workspaceDeleteBtn.addEventListener("click", () => {
      deleteSelectedWorkspace();
    });
  }
  if (workspacePullBtn) {
    workspacePullBtn.addEventListener("click", async () => {
      await fetchSnapshotForActive();
      await pollActivity();
      setToolStatus(`Pulled workspace ${state.runtime.activeWorkspace}.`);
    });
  }
  if (workspacePushBtn) {
    workspacePushBtn.addEventListener("click", async () => {
      const snapshotJson = currentNetworkJson();
      const configJson = currentConfigJson();
      const payload = snapshotJson || configJson;
      if (!payload) {
        setToolStatus("No snapshot or config is loaded to push.");
        return;
      }
      const kind = snapshotJson ? "snapshot" : "config";
      const ok = await importWorkspacePayload(payload, kind, {
        replaceBaseline: true,
      });
      if (ok) {
        setToolStatus(`Pushed ${kind} into workspace ${state.runtime.activeWorkspace}.`);
      }
    });
  }
  if (workspaceStartBtn) {
    workspaceStartBtn.addEventListener("click", () => {
      controlWorkspaceAction("start");
    });
  }
  if (workspaceStopBtn) {
    workspaceStopBtn.addEventListener("click", () => {
      controlWorkspaceAction("stop");
    });
  }
  layoutButtons.forEach((btn) => {
    btn.addEventListener("click", () => {
      setLayout(btn.dataset.layout);
    });
  });

  fullTopologyToggle.addEventListener("change", () => {
    state.render.fullTopology = fullTopologyToggle.checked;
    saveRenderSettings();
    fetchSnapshotForActive();
  });

  edgeLimitInput.addEventListener("input", () => {
    state.render.edgeLimit = Number(edgeLimitInput.value);
    edgeLimitValue.textContent = edgeLimitInput.value;
  });
  edgeLimitInput.addEventListener("change", () => {
    saveRenderSettings();
    fetchSnapshotForActive();
  });

  weightThresholdInput.addEventListener("input", () => {
    state.render.weightThreshold = Number(weightThresholdInput.value);
    weightThresholdValue.textContent = Number(weightThresholdInput.value).toFixed(2);
  });
  weightThresholdInput.addEventListener("change", () => {
    saveRenderSettings();
    fetchSnapshotForActive();
  });

  showRegionLabelsInput.addEventListener("change", () => {
    state.render.showRegionLabels = showRegionLabelsInput.checked;
    saveRenderSettings();
    drawNetwork();
  });

  if (probeSourceInput) {
    probeSourceInput.addEventListener("change", syncProbeControls);
  }
  if (probeLayerInput) {
    probeLayerInput.addEventListener("input", syncProbeControls);
  }
  if (probeIndexInput) {
    probeIndexInput.addEventListener("input", syncProbeControls);
  }
  if (addProbeBtn) {
    addProbeBtn.addEventListener("click", () => {
      syncProbeControls();
      const targetType = probeSourceInput ? probeSourceInput.value || "sensory" : "sensory";
      const layer = targetType === "hidden" ? Number(probeLayerInput?.value || 0) : 0;
      const index = Number(probeIndexInput?.value || 0);
      addProbe({ targetType, layer, index });
    });
  }
  if (clearProbesBtn) {
    clearProbesBtn.addEventListener("click", () => {
      resetInstrumentationBuffers({ keepProbes: false });
      renderInstrumentation();
      setToolStatus("Cleared all probes.");
    });
  }
  if (saveConfigBtn) {
    saveConfigBtn.addEventListener("click", () => {
      const raw = currentConfigJson();
      if (!raw) {
        setToolStatus("No config available to save yet.");
        return;
      }
      downloadTextFile("config.json", raw);
      setToolStatus("Saved current config to config.json.");
    });
  }
  if (loadConfigBtn) {
    loadConfigBtn.addEventListener("click", async () => {
      const raw = await pickJsonFile();
      if (!raw) {
        setToolStatus("No config file selected.");
        return;
      }
      applyRemoteJsonPayload(raw, "Config");
    });
  }
  if (saveNetworkBtn) {
    saveNetworkBtn.addEventListener("click", () => {
      const raw = currentNetworkJson();
      if (!raw) {
        setToolStatus("No snapshot available to save yet.");
        return;
      }
      downloadTextFile("network.json", raw);
      setToolStatus("Saved current network snapshot to network.json.");
    });
  }
  if (loadNetworkBtn) {
    loadNetworkBtn.addEventListener("click", async () => {
      const raw = await pickJsonFile();
      if (!raw) {
        setToolStatus("No network file selected.");
        return;
      }
      applyRemoteJsonPayload(raw, "Network snapshot");
    });
  }
  if (saveProbesBtn) {
    saveProbesBtn.addEventListener("click", () => {
      downloadTextFile(
        "probes.json",
        JSON.stringify({ probes: serializeProbes() }, null, 2)
      );
      setToolStatus("Saved local probe set to probes.json.");
    });
  }
  if (loadProbesBtn) {
    loadProbesBtn.addEventListener("click", async () => {
      const raw = await pickJsonFile();
      if (!raw) {
        setToolStatus("No probe file selected.");
        return;
      }
      try {
        const parsed = JSON.parse(raw);
        const probes = Array.isArray(parsed) ? parsed : Array.isArray(parsed?.probes) ? parsed.probes : null;
        if (!probes) {
          setToolStatus("Probe file must be an array or an object with a probes array.");
          return;
        }
        state.instrumentation.probes = probes.map((probe, idx) => normalizeProbe(probe, idx + 1));
        state.instrumentation.nextProbeId =
          state.instrumentation.probes.reduce((maxId, probe) => Math.max(maxId, probe.id), 0) + 1;
        state.instrumentation.probes.forEach((probe) => {
          probe.samples = [];
        });
        saveInstrumentationState();
        renderInstrumentation();
        setToolStatus("Loaded local probe set.");
      } catch (_) {
        setToolStatus("Probe file is not valid JSON.");
      }
    });
  }
}

function attachCanvasControls() {
  let dragging = false;
  let lastX = 0;
  let lastY = 0;
  let mode = "pan";
  canvas.addEventListener("contextmenu", (e) => {
    e.preventDefault();
    const target = findNearestGraphNode(e.clientX, e.clientY);
    if (!target) {
      hideGraphContextMenu();
      return;
    }
    showGraphContextMenu(target, e.clientX, e.clientY);
  });
  canvas.addEventListener("pointerdown", (e) => {
    hideGraphContextMenu();
    if (e.button === 2) return;
    dragging = true;
    lastX = e.clientX;
    lastY = e.clientY;
    const allowRotate = state.render.layout !== "conventional";
    mode = allowRotate && e.ctrlKey ? "rotate" : "pan";
    canvas.setPointerCapture(e.pointerId);
    canvas.style.cursor = mode === "pan" ? "grabbing" : "crosshair";
  });
  canvas.addEventListener("pointerup", (e) => {
    if (!dragging) return;
    dragging = false;
    canvas.releasePointerCapture(e.pointerId);
    canvas.style.cursor = "grab";
  });
  canvas.addEventListener("pointermove", (e) => {
    if (!dragging) return;
    const dx = e.clientX - lastX;
    const dy = e.clientY - lastY;
    lastX = e.clientX;
    lastY = e.clientY;
    if (mode === "pan") {
      state.view.offsetX += dx;
      state.view.offsetY += dy;
    } else {
      state.view.rotation += dx * 0.005;
    }
    drawNetwork();
  });
  canvas.addEventListener("wheel", (e) => {
    e.preventDefault();
    const delta = Math.sign(e.deltaY);
    state.view.zoom = Math.min(2.5, Math.max(0.4, state.view.zoom - delta * 0.05));
    drawNetwork();
  });
}

if (loginForm) {
  loginForm.addEventListener("submit", (e) => {
    e.preventDefault();
    performLogin(loginUsername.value.trim(), loginPassword.value);
  });
}
if (signupBtn) {
  signupBtn.addEventListener("click", () => {
    performSignup(loginUsername.value.trim(), loginPassword.value);
  });
}
if (logoutBtn) {
  logoutBtn.addEventListener("click", () => {
    performLogout();
  });
}
if (graphAddProbeBtn) {
  graphAddProbeBtn.addEventListener("click", () => {
    const target = state.instrumentation.contextTarget;
    if (!target) return;
    const existing = findProbeByTarget(target);
    if (existing) {
      state.instrumentation.probes = state.instrumentation.probes.filter(
        (probe) => !probeMatches(probe, target)
      );
      saveInstrumentationState();
      renderInstrumentation();
      setToolStatus(`Removed probe ${formatGraphTarget(target)}.`);
    } else {
      addProbe(target);
    }
    hideGraphContextMenu();
  });
}
window.addEventListener("click", (event) => {
  if (!graphContextMenu || graphContextMenu.classList.contains("hidden")) return;
  if (graphContextMenu.contains(event.target)) return;
  hideGraphContextMenu();
});

async function boot() {
  await initAuth();
  await loadRuntimeStatus();
  await initTargets();
  resizeCanvas();
  attachControls();
  attachCanvasControls();
  renderInstrumentation();
  refreshNetworkSelect();
  if (isWorkspaceMode()) {
    await fetchSnapshotForActive();
    await pollActivity();
  }
}

boot();

if (startStopBtn) {
  startStopBtn.addEventListener("click", () => {
    const action = getActivePlaying() ? "stop" : "start";
    sendControlAction(action);
  });
}
if (repeatBtn) {
  repeatBtn.addEventListener("click", () => sendControlAction("repeat"));
}
if (resetBtn) {
  resetBtn.addEventListener("click", () => sendControlAction("reset"));
}
if (newBtn) {
  newBtn.addEventListener("click", () => sendControlAction("new"));
}
  
[modelSelector, learningSelector].forEach(selector => {
  selector.querySelectorAll("button").forEach(btn => {
    btn.addEventListener("click", () => {
      selector.querySelectorAll("button").forEach(b => b.classList.remove("active"));
      btn.classList.add("active");
      updateNetworkSettings();
    });
  });
});
  
[aarnnRandomness, aarnnDepth].forEach(input => {
  input.addEventListener("input", () => {
    if (input === aarnnRandomness) aarnnRandomnessValue.textContent = parseFloat(input.value).toFixed(2);
    if (input === aarnnDepth) aarnnDepthValue.textContent = input.value;
  });
  input.addEventListener("change", updateNetworkSettings);
});
  
[useDelays, useMorphology, useStp, useNeuromod, evolution3d, growth3dInput].forEach(input => {
  input.addEventListener("change", (e) => {
    if (input === evolution3d) growth3dInput.checked = e.target.checked;
    if (input === growth3dInput) evolution3d.checked = e.target.checked;
    updateNetworkSettings();
  });
});

clumpingDesign.addEventListener("change", updateNetworkSettings);
  
resetBioBtn.addEventListener("click", () => {
  // Biologically plausible defaults matching Rust UI
  updateSegmentedSelector(modelSelector, "aarnn");
  updateSegmentedSelector(learningSelector, "aarnn");
  aarnnRandomness.value = 0.1;
  aarnnRandomnessValue.textContent = "0.10";
  aarnnDepth.value = 5;
  aarnnDepthValue.textContent = "5";
  useDelays.checked = true;
  useMorphology.checked = true;
  useStp.checked = true;
  useNeuromod.checked = true;
  evolution3d.checked = true;
  growth3dInput.checked = true;
  clumpingDesign.value = "HumanBrain";
  updateNetworkSettings({ forceBaseline: true });
});

async function exportModel(format) {
  if (isWorkspaceMode()) {
    const workspace = getActiveWorkspaceMeta();
    if (!workspace) return;
    const url = `/api/runtime/workspaces/${encodeURIComponent(workspace.workspace_id)}/export?format=${encodeURIComponent(
      format
    )}`;
    window.open(url, "_blank");
    return;
  }
  if (!state.active || !state.activeNetwork) return;
  const url = `/api/export?addr=${encodeURIComponent(state.active)}&network_id=${encodeURIComponent(state.activeNetwork)}&format=${format}`;
  window.open(url, '_blank');
}

if (exportNeuromlBtn) exportNeuromlBtn.addEventListener("click", () => exportModel("neuroml"));
if (exportPynnBtn) exportPynnBtn.addEventListener("click", () => exportModel("pynn"));
if (exportNirBtn) exportNirBtn.addEventListener("click", () => exportModel("nir"));
if (exportOnnxBtn) exportOnnxBtn.addEventListener("click", () => exportModel("onnx"));
if (exportTfliteBtn) exportTfliteBtn.addEventListener("click", () => exportModel("tflite"));

setInterval(() => {
  loadRuntimeStatus();
}, POLL_MS);
setInterval(pollAll, POLL_MS);
setInterval(pollActivity, ACTIVITY_POLL_MS);
setInterval(pollSnapshot, SNAPSHOT_POLL_TICK_MS);
