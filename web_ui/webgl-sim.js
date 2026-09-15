/* AARNN WebGL simulator. Rendering and profile adapters live in the browser;
 * neural execution remains behind the authenticated /api/aer/infer gateway. */
(function () {
  "use strict";

  var PROFILES = {};
  window.NmSimContent.profiles.forEach(function (p) { PROFILES[p.id] = p; });
  var worldRenderer = null;
  var sensoryHistory = {};
  var habitat = null;
  var sessionGeneration = 0;
  var robotSelect = document.getElementById("webgl-robot");
  var networkInput = document.getElementById("webgl-network");
  var nodeInput = document.getElementById("webgl-node");
  var connectButton = document.getElementById("webgl-connect");
  var pauseButton = document.getElementById("webgl-pause");
  var controlSurfaceLink = document.getElementById("webgl-control-surface-link");
  var canvas = document.getElementById("webgl-canvas");
  var message = document.getElementById("webgl-message");
  var transport = document.getElementById("webgl-transport");
  var capability = document.getElementById("webgl-capability");
  var stepText = document.getElementById("webgl-step");
  var inputSpikeText = document.getElementById("webgl-input-spikes");
  var outputSpikeText = document.getElementById("webgl-output-spikes");
  var sensoryText = document.getElementById("webgl-sensory");
  var outputText = document.getElementById("webgl-output");
  var brainText = document.getElementById("webgl-brain");
  var targetText = document.getElementById("webgl-target");
  var gl = canvas && (canvas.getContext("webgl", { antialias: true }) || canvas.getContext("experimental-webgl"));
  var profile = PROFILES.celegans;
  var running = true;
  var connected = false;
  var frame = 0;
  var lastTime = 0;
  var requestInFlight = false;
  var requestController = null;
  var robot = { x: 0, z: 0, heading: 0, phase: 0, actuators: [] };
  var aerSession = null;
  var routeParams = new URLSearchParams(window.location.search || "");
  var routeNetwork = (routeParams.get("network_id") || "").trim();
  var routeNode = (routeParams.get("node_id") || "").trim();
  var routeRobot = (routeParams.get("robot") || "").trim();
  var socialMode = routeParams.get("nao_social") === "1";
  var bodySession = "", bodyToken = "", bodyOpening = false;
  var bubbleTimer = null;
  if (socialMode) {
    document.body.classList.add('nao-social');
    document.querySelector('.subtitle').textContent = 'Explore the room and talk with NAO’s small neural social circuit.';
    document.getElementById("nao-social-setup").hidden = false;
    var chatWindow = document.getElementById("nao-chat-window");
    chatWindow.hidden = false; chatWindow.src = "/";
    document.querySelector('.webgl-layout').appendChild(chatWindow);
    robotSelect.value = "nao"; robotSelect.disabled = true;
    networkInput.value = "nao"; networkInput.disabled = true;
    nodeInput.disabled = true;
    window.addEventListener("message", function (event) {
      if (event.origin !== window.location.origin || event.source !== chatWindow.contentWindow || !event.data || event.data.type !== "nao-reply") return;
      var reply = event.data.reply;
      if (!connected || !reply || typeof reply.text !== "string" || reply.text.length > 512) return;
      var bubble = document.getElementById("nao-bubble"); bubble.textContent = "NAO: " + reply.text; bubble.hidden = false;
      clearTimeout(bubbleTimer); bubbleTimer = setTimeout(function () { bubble.hidden = true; }, 10000);
    });
  }
  if (routeNetwork) networkInput.value = routeNetwork;
  if (routeNode) nodeInput.value = routeNode;
  if (routeRobot && PROFILES[routeRobot]) robotSelect.value = routeRobot;
  if (socialMode) { robotSelect.value = 'nao'; networkInput.value = 'nao'; nodeInput.value = ''; }
  // A focused player avatar is a visible/contact cue for every profile. Arrow
  // keys move it only while the simulation canvas has keyboard focus.
  robot.participants = [{id:'local-player', position:[.3,0,.14], size:[.06,.06,.28]}];
  canvas.tabIndex = 0;
  canvas.setAttribute('aria-label', 'Robot world. Shift and arrow keys move your blue avatar when this view is focused.');
  canvas.addEventListener('keydown', function (event) {
    var delta = {ArrowUp:[.03,0], ArrowDown:[-.03,0], ArrowLeft:[0,.03], ArrowRight:[0,-.03]}[event.key];
    if (!delta || !event.shiftKey) return;
    event.preventDefault(); var p = robot.participants[0].position;
    p[0] = clamp(p[0]+delta[0],-.85,.85); p[1] = clamp(p[1]+delta[1],-.85,.85);
  });

  function setText(element, value) { if (element) element.textContent = String(value); }
  function clamp(value, low, high) { return Math.max(low, Math.min(high, value)); }
  function setMessage(text, className) {
    if (!message) return;
    message.textContent = text;
    message.className = "webgl-note" + (className ? " " + className : "");
  }
  function syncControlSurfaceLink() {
    if (!controlSurfaceLink) return;
    var params = new URLSearchParams();
    if (networkInput.value.trim()) params.set("network_id", networkInput.value.trim());
    if (nodeInput.value.trim()) params.set("node_id", nodeInput.value.trim());
    var query = params.toString();
    controlSurfaceLink.href = query ? "/?" + query : "/";
  }
  function resize() { if (worldRenderer) worldRenderer.render(robot); }
  function initGl() { worldRenderer = new window.NmWorld.Renderer(gl, canvas); }
  function render() { if (worldRenderer) worldRenderer.render(robot); }
  function updateProfile() {
    profile = PROFILES[robotSelect.value] || PROFILES.celegans;
    habitat = window.NmSimContent.habitats.filter(function (h) { return h.id === profile.habitat; })[0];
    sensoryHistory = {};
    robot.x = 0; robot.z = 0; robot.heading = 0;
    if (worldRenderer) worldRenderer.setProfile(profile, habitat);
    setText(document.getElementById("webgl-habitat"), habitat.title);
    setText(document.getElementById("webgl-biology"), profile.biology_note);
    setText(document.getElementById("webgl-cues"), habitat.cues.join(" · "));
    setText(document.getElementById("webgl-anatomy-description"), profile.anatomy.join(" · "));
    setText(document.getElementById("webgl-content-version"), "v1 / " + window.NmSimContent.digest.slice(0, 12));
    robot.actuators = []; for (var i = 0; i < profile.output; i += 1) robot.actuators.push(0);
    setText(sensoryText, profile.sensory); setText(outputText, profile.output); setText(brainText, networkInput.value || "(unset)");
    setText(targetText, nodeInput.value.trim() || "auto");
  }
  function sensors() {
    return window.NmWorld.sense(profile, habitat, robot, sensoryHistory);
  }
  function applyOutputs(indices) {
    var i;
    for (i = 0; i < robot.actuators.length; i += 1) robot.actuators[i] *= 0.82;
    (indices || []).forEach(function (index) {
      var channel = Number(index);
      if (Number.isInteger(channel) && channel >= 0 && channel < profile.output) robot.actuators[channel] = 1;
    });
    var movement = window.NmWorld.motion(profile, robot.actuators);
    var mean = movement.drive;
    robot.heading += movement.turn * 0.08;
    var x = robot.x + Math.cos(robot.heading) * mean * 0.008;
    var z = robot.z + Math.sin(robot.heading) * mean * 0.008;
    var hit = window.NmWorld.ray(habitat, [robot.x, robot.z, profile.body_height], [Math.cos(robot.heading), Math.sin(robot.heading), 0], profile.body_length * 0.65);
    if (hit.distance >= profile.body_length * 0.6) { robot.x = clamp(x, -.85, .85); robot.z = clamp(z, -.85, .85); }
    if (worldRenderer) worldRenderer.bodyDirty = true;
  }
  function makeSession() {
    if (!window.AARNNBrowserAerSession) return null;
    var seed = Date.now() % 2147483647;
    return new window.AARNNBrowserAerSession(seed || 1, 1, 1, 32);
  }
  function infer() {
    if (!connected || requestInFlight || !running || !networkInput.value.trim()) return;
    var generation = sessionGeneration;
    requestInFlight = true; var values = sensors(); var active = values.filter(function (value) { return value > 0.55; }).length;
    var sessionFrame = null;
    try {
      if (aerSession) sessionFrame = aerSession.nextFrame({ source_sequence: frame, capture_timestamp_ns: frame * 1000000, clock_mapping_version: 1, clock_uncertainty_ns: 1, address_space_version: 1, direction: "producer", polarity: true, payload_type: "events", payload: [] });
    } catch (error) {
      requestInFlight = false; transport.className = "webgl-error"; transport.textContent = "frame error"; setMessage(error.message, "webgl-error"); return;
    }
    setText(inputSpikeText, active); transport.className = "webgl-ok"; transport.textContent = "sending";
    var body = { network_id: networkInput.value.trim(), node_id: nodeInput.value.trim() || null, step_index: frame, time_ms: frame, dt_ms: 1, input_values: values, timeout_ms: 250, aer_frame_sequence: sessionFrame ? sessionFrame.frame_sequence : null };
    requestController = new AbortController();
    var controller = requestController;
    var deadline = window.setTimeout(function () { controller.abort(); }, 2000);
    if (socialMode) body.body_session = bodySession;
    var headers = { "Content-Type": "application/json" };
    if (socialMode) headers.Authorization = "Bearer " + bodyToken;
    fetch(socialMode ? "/api/body" : "/api/aer/infer", { method: "POST", credentials: "same-origin", signal: controller.signal, headers: headers, body: JSON.stringify(body) })
      .then(function (response) { return response.json().then(function (payload) { if (!response.ok) throw new Error(payload.error || "inference request failed"); return payload; }); })
      .then(function (payload) { if (generation !== sessionGeneration) return; frame += 1; var indices = Array.isArray(payload.output_spike_indices) ? payload.output_spike_indices : []; applyOutputs(indices); setText(outputSpikeText, indices.length); if (payload.output_step_index != null) setText(stepText, payload.output_step_index); if (aerSession) aerSession.acknowledge(1); transport.textContent = "connected"; })
      .catch(function (error) { if (generation !== sessionGeneration) return; disconnect(); transport.className = "webgl-error"; transport.textContent = "error"; setMessage(error.message + " Rendering continues in preview mode.", "webgl-error"); })
      .then(function () { window.clearTimeout(deadline); if (requestController === controller) requestController = null; requestInFlight = false; });
  }
  function tick(now) {
    if (!lastTime) lastTime = now; var dt = Math.min(0.1, (now - lastTime) / 1000); lastTime = now;
    if (running) { infer(); }
    render(); window.requestAnimationFrame(tick);
  }
  if (!gl) { capability.className = "webgl-note webgl-error"; capability.textContent = "WebGL unavailable"; setMessage("This browser does not expose WebGL. The neural runtime is unaffected; use Webots, Unity, or Unreal for rendered simulation.", "webgl-error"); }
  else { capability.className = "webgl-note webgl-ok"; capability.textContent = "WebGL available"; try { initGl(); } catch (error) { capability.textContent = "WebGL shader error"; setMessage(error.message, "webgl-error"); } }
  function disconnect() {
    if (bodySession) {
      fetch("/api/body/close", { method: "POST", headers: {"Content-Type":"application/json", "Authorization":"Bearer "+bodyToken}, body:JSON.stringify({body_session:bodySession}), keepalive:true }).catch(function () {});
      bodySession = "";
    }
    var bubble = document.getElementById("nao-bubble"); if (bubble) bubble.hidden = true;
    connected = false; sessionGeneration += 1;
    if (requestController) requestController.abort();
    robot.actuators.fill(0); if (worldRenderer) worldRenderer.bodyDirty = true;
    transport.textContent = "disconnected"; connectButton.textContent = "Connect brain";
  }
  robotSelect.addEventListener("change", function () { disconnect(); updateProfile(); });
  networkInput.addEventListener("input", function () { disconnect(); updateProfile(); syncControlSurfaceLink(); });
  nodeInput.addEventListener("input", function () { disconnect(); updateProfile(); syncControlSurfaceLink(); });
  connectButton.addEventListener("click", function () {
    if (connected) { disconnect(); return; }
    if (bodyOpening) return;
    function ready() { connected = true; connectButton.textContent = "Disconnect"; aerSession = makeSession(); transport.className = "webgl-ok"; transport.textContent = "connected"; setMessage(socialMode ? "Local NAO social reference session active." : "Sensory frames are being admitted through the distributed AER gateway.", "webgl-ok"); }
    if (!socialMode) { ready(); return; }
    bodyToken = document.getElementById("nao-body-token").value.trim();
    var generation = sessionGeneration; bodyOpening = true;
    fetch("/api/body/open", {method:"POST", headers:{"Content-Type":"application/json", "Authorization":"Bearer "+bodyToken}, body:"{}"})
      .then(function (response) { return response.json().then(function (data) { if (!response.ok) throw Error(data.error || "NAO body unavailable"); return data; }); })
      .then(function (data) { bodySession = data.body_session; if (generation !== sessionGeneration) { disconnect(); return; } ready(); })
      .catch(function (error) { setMessage(error.message, "webgl-error"); })
      .finally(function () { bodyOpening = false; });
  });
  pauseButton.addEventListener("click", function () { running = !running; if (!running) disconnect(); pauseButton.textContent = running ? "Pause" : "Resume"; });
  document.getElementById("webgl-anatomy").addEventListener("change", function (event) { if (worldRenderer) { worldRenderer.anatomy = event.target.checked; worldRenderer.bodyDirty = true; if (event.target.checked) { worldRenderer.focusRobot = true; worldRenderer.distance = .5; } } });
  document.getElementById("webgl-focus-robot").addEventListener("click", function () { if (worldRenderer) { worldRenderer.focusRobot = true; worldRenderer.distance = .5; } });
  document.getElementById("webgl-reset-view").addEventListener("click", function () { if (worldRenderer) { worldRenderer.focusRobot = false; worldRenderer.yaw = -1.25; worldRenderer.pitch = .83; worldRenderer.distance = 2.65; } });
  document.addEventListener("visibilitychange", function () { if (document.hidden) disconnect(); });
  window.addEventListener("pagehide", disconnect);
  window.addEventListener("resize", resize);
  updateProfile();
  syncControlSurfaceLink();
  if (!routeNetwork) {
    fetch("/api/config", { credentials: "same-origin" }).then(function (response) {
      return response.ok ? response.json() : null;
    }).then(function (config) {
      if (!config) return;
      if (!routeNetwork && config.default_network) networkInput.value = String(config.default_network);
      if (!routeNode && config.default_node) nodeInput.value = String(config.default_node);
      updateProfile();
      syncControlSurfaceLink();
    }).catch(function () {});
  }
  window.requestAnimationFrame(tick);
}());
