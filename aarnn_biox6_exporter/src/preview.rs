use crate::calibration::CalibrationSet;
use crate::config::MachineProfile;
use crate::model::{EdgeImplementation, NodeKind, NodeRealisation, PhysicalPlan, Polarity, Vec3};
use serde_json::json;

pub fn render_preview_svg(plan: &PhysicalPlan, machine: &MachineProfile) -> String {
    let width = machine.build_volume_mm.x.max(1.0);
    let height = machine.build_volume_mm.y.max(1.0);
    let mut svg = String::new();
    svg.push_str(&format!(
        r#"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 {width:.3} {height:.3}" width="{width:.0}mm" height="{height:.0}mm" role="img" aria-label="AARNN BIO X6 print preview">"#
    ));
    svg.push_str(r##"<rect x="0" y="0" width="100%" height="100%" fill="#f8fafc"/>"##);
    svg.push_str(&format!(
        r##"<rect x="0.5" y="0.5" width="{:.3}" height="{:.3}" fill="none" stroke="#334155" stroke-width="0.6"/>"##,
        (width - 1.0).max(0.0),
        (height - 1.0).max(0.0)
    ));

    for edge in &plan.connections {
        if edge.path_mm.len() < 2 {
            continue;
        }
        let color = match edge.polarity {
            crate::model::Polarity::Excitatory => "#16a34a",
            crate::model::Polarity::Inhibitory => "#dc2626",
        };
        let dash = if edge.residual_delay_us > 0.001 {
            r#" stroke-dasharray="2 1""#
        } else {
            ""
        };
        let points = edge
            .path_mm
            .iter()
            .map(|p| format!("{:.3},{:.3}", p.x, height - p.y))
            .collect::<Vec<_>>()
            .join(" ");
        svg.push_str(&format!(
            r#"<polyline points="{points}" fill="none" stroke="{color}" stroke-width="{:.3}" opacity="0.82"{dash}><title>{}: {} -> {}, z {:.2}-{:.2} mm, residual delay {:.6} us</title></polyline>"#,
            edge.core_diameter_mm.max(0.25),
            xml_escape(&edge.id),
            xml_escape(&edge.source_node),
            xml_escape(&edge.target_node),
            min_z(&edge.path_mm),
            max_z(&edge.path_mm),
            edge.residual_delay_us,
        ));
    }

    for node in &plan.nodes {
        let p = node.position_mm;
        let radius = node.radius_mm.max(0.8);
        svg.push_str(&format!(
            r##"<circle cx="{:.3}" cy="{:.3}" r="{:.3}" fill="#2563eb" stroke="#0f172a" stroke-width="0.25"><title>{}: {:?}, threshold {:.6} mW</title></circle>"##,
            p.x,
            height - p.y,
            radius,
            xml_escape(&node.id),
            node.realisation,
            node.realised_threshold_mw,
        ));
    }

    svg.push_str(
        r##"<g font-family="Inter, Arial, sans-serif" font-size="3.4" fill="#0f172a">
<rect x="3" y="3" width="72" height="18" rx="1.5" fill="#ffffff" stroke="#cbd5e1" stroke-width="0.3"/>
<text x="5" y="8">BIO X6 preview: top-down XY projection</text>
<text x="5" y="13" fill="#16a34a">green excitatory</text>
<text x="5" y="18" fill="#dc2626">red inhibitory; dashed needs non-geometric delay</text>
</g>"##,
    );
    svg.push_str("</svg>");
    svg
}

pub fn render_preview_html(
    plan: &PhysicalPlan,
    machine: &MachineProfile,
    calibration: &CalibrationSet,
) -> String {
    let preview_data = html_script_json(&preview_data_json(plan, machine));
    let warnings = if plan.warnings.is_empty() {
        "<li>No validation warnings were recorded for the physical plan.</li>".to_string()
    } else {
        plan.warnings
            .iter()
            .map(|w| {
                format!(
                    "<li><code>{}</code>{}: {}</li>",
                    html_escape(&w.code),
                    w.entity_id
                        .as_ref()
                        .map(|id| format!(" <span>{}</span>", html_escape(id)))
                        .unwrap_or_default(),
                    html_escape(&w.message)
                )
            })
            .collect::<Vec<_>>()
            .join("\n")
    };

    let mut html = String::new();
    html.push_str(
        r##"<!doctype html>
<html lang="en">
<head>
  <meta charset="utf-8" />
  <meta name="viewport" content="width=device-width, initial-scale=1" />
  <title>AARNN BIO X6 Print Preview</title>
  <style>
    :root { color-scheme: light; font-family: Inter, Arial, sans-serif; }
    * { box-sizing: border-box; }
    body { margin: 0; background: #f3f4f1; color: #172016; }
    main { max-width: 1240px; margin: 0 auto; padding: 24px; }
    header { display: flex; gap: 16px; align-items: baseline; justify-content: space-between; flex-wrap: wrap; }
    h1 { font-size: 24px; margin: 0 0 4px; }
    h2 { font-size: 18px; margin: 18px 0 8px; }
    button { border: 1px solid #a8b3a2; background: #ffffff; border-radius: 6px; color: #172016; cursor: pointer; font: inherit; padding: 7px 10px; }
    button:hover { background: #eef4ea; }
    input[type="range"] { width: min(180px, 100%); accent-color: #2f7d52; }
    code { font-family: ui-monospace, SFMono-Regular, Menlo, monospace; }
    .muted { color: #596456; }
    .preview-panel { margin-top: 18px; background: white; border: 1px solid #c9d1c4; border-radius: 8px; overflow: hidden; }
    .preview-toolbar { display: grid; gap: 10px; padding: 12px; border-bottom: 1px solid #dce3d8; background: #fbfcf9; }
    .button-row, .control-row { display: flex; gap: 8px; align-items: center; flex-wrap: wrap; }
    .control-row label { display: grid; gap: 4px; color: #3e493a; font-size: 13px; min-width: 138px; }
    .status { margin-left: auto; color: #596456; font-size: 13px; }
    .preview-stage { height: min(72vh, 720px); min-height: 420px; background: #f8faf6; }
    .preview-stage svg { display: block; width: 100%; height: 100%; touch-action: none; cursor: grab; user-select: none; }
    .preview-stage svg.rotating { cursor: move; }
    .preview-help { margin: 0; padding: 9px 12px 12px; border-top: 1px solid #dce3d8; color: #596456; font-size: 13px; }
    .grid { display: grid; grid-template-columns: repeat(auto-fit, minmax(220px, 1fr)); gap: 12px; margin-top: 16px; }
    .metric { background: white; border: 1px solid #d7ded2; border-radius: 8px; padding: 12px; }
    .metric strong { display: block; font-size: 13px; color: #596456; margin-bottom: 4px; }
    ul { background: white; border: 1px solid #d7ded2; border-radius: 8px; padding: 12px 12px 12px 28px; }
  </style>
</head>
<body>
"##,
    );
    html.push_str(&format!(
        r##"<main>
  <header>
    <div>
      <h1>AARNN BIO X6 Print Preview</h1>
      <div class="muted">Network <code>{network}</code>, tick {tick}, calibration <code>{calibration_id}</code></div>
    </div>
    <div class="muted">{machine_model}</div>
  </header>
"##,
        network = html_escape(&plan.source_network_id),
        tick = plan.source_tick,
        calibration_id = html_escape(&calibration.id),
        machine_model = html_escape(&machine.model),
    ));
    html.push_str(
        r##"  <section class="preview-panel" aria-label="Interactive BIO X6 print preview">
    <div class="preview-toolbar">
      <div class="button-row">
        <button type="button" id="view-reset">Reset</button>
        <button type="button" id="view-top">Top</button>
        <button type="button" id="view-front">Front</button>
        <button type="button" id="view-iso">Iso</button>
        <span class="status" id="view-status"></span>
      </div>
      <div class="control-row">
        <label>Yaw
          <input id="view-yaw" type="range" min="-180" max="180" step="1" value="35" />
        </label>
        <label>Tilt
          <input id="view-pitch" type="range" min="-90" max="90" step="1" value="55" />
        </label>
        <label>Roll
          <input id="view-roll" type="range" min="-180" max="180" step="1" value="0" />
        </label>
        <label>Zoom
          <input id="view-zoom" type="range" min="25" max="800" step="5" value="100" />
        </label>
      </div>
    </div>
    <div class="preview-stage">
      <svg id="preview-svg" viewBox="0 0 1000 680" role="img" aria-label="Interactive 3D BIO X6 model preview">
        <rect width="1000" height="680" fill="#f8faf6"></rect>
        <g id="preview-scene"></g>
      </svg>
    </div>
    <p class="preview-help">Wheel zoom. Drag to pan. Shift-drag, right-drag, or middle-drag to rotate and tilt the 3D model.</p>
  </section>
"##,
    );
    html.push_str(&format!(
        r##"  <section class="grid">
    <div class="metric"><strong>Nodes</strong>{nodes}</div>
    <div class="metric"><strong>Connections</strong>{connections}</div>
    <div class="metric"><strong>Total path</strong>{path:.3} mm</div>
    <div class="metric"><strong>Max residual delay</strong>{delay:.6} us</div>
    <div class="metric"><strong>Min transmission</strong>{transmission:.6}</div>
    <div class="metric"><strong>Core volume estimate</strong>{core_volume:.6} ml</div>
  </section>
  <h2>Validation Notes</h2>
  <ul>{warnings}</ul>
  <p class="muted">This preview renders the physical implementation plan as a rotatable SVG projection of the 3D build volume. Z-layer crossings and residual delay strategies are encoded in the generated physical plan and toolpaths; verify the job in DNA Studio before any machine run.</p>
  <script id="preview-data" type="application/json">"##,
        nodes = plan.nodes.len(),
        connections = plan.connections.len(),
        path = plan.metrics.total_path_length_mm,
        delay = plan.metrics.maximum_delay_error_us,
        transmission = plan.metrics.minimum_estimated_transmission,
        core_volume = plan.metrics.estimated_core_volume_ml,
        warnings = warnings,
    ));
    html.push_str(&preview_data);
    html.push_str(
        r##"</script>
  <script>
(() => {
  "use strict";

  const SVG_NS = "http://www.w3.org/2000/svg";
  const data = JSON.parse(document.getElementById("preview-data").textContent);
  const svg = document.getElementById("preview-svg");
  const scene = document.getElementById("preview-scene");
  const status = document.getElementById("view-status");
  const controls = {
    yaw: document.getElementById("view-yaw"),
    pitch: document.getElementById("view-pitch"),
    roll: document.getElementById("view-roll"),
    zoom: document.getElementById("view-zoom")
  };
  const viewport = { width: 1000, height: 680 };
  const machine = data.machine;
  const center = {
    x: machine.widthMm / 2,
    y: machine.depthMm / 2,
    z: machine.heightMm / 2
  };
  const span = Math.max(machine.widthMm, machine.depthMm, machine.heightMm, 1);
  const state = { yaw: 35, pitch: 55, roll: 0, zoom: 1, panX: 0, panY: 0 };
  let drag = null;

  function clamp(value, min, max) {
    return Math.max(min, Math.min(max, value));
  }

  function radians(degrees) {
    return degrees * Math.PI / 180;
  }

  function point(tuple) {
    return { x: tuple[0], y: tuple[1], z: tuple[2] };
  }

  function currentScale() {
    return Math.min(viewport.width, viewport.height) * 0.72 / span * state.zoom;
  }

  function rotate(p) {
    let x = p.x - center.x;
    let y = p.y - center.y;
    let z = p.z - center.z;

    const yaw = radians(state.yaw);
    const cosYaw = Math.cos(yaw);
    const sinYaw = Math.sin(yaw);
    const xYaw = x * cosYaw - y * sinYaw;
    const yYaw = x * sinYaw + y * cosYaw;

    const pitch = radians(state.pitch);
    const cosPitch = Math.cos(pitch);
    const sinPitch = Math.sin(pitch);
    const yPitch = yYaw * cosPitch - z * sinPitch;
    const zPitch = yYaw * sinPitch + z * cosPitch;

    const roll = radians(state.roll);
    const cosRoll = Math.cos(roll);
    const sinRoll = Math.sin(roll);
    return {
      x: xYaw * cosRoll + zPitch * sinRoll,
      y: yPitch,
      z: -xYaw * sinRoll + zPitch * cosRoll
    };
  }

  function project(p) {
    const rotated = rotate(p);
    const scale = currentScale();
    return {
      x: viewport.width / 2 + state.panX + rotated.x * scale,
      y: viewport.height / 2 + state.panY - rotated.y * scale,
      z: rotated.z
    };
  }

  function el(name, attributes = {}) {
    const node = document.createElementNS(SVG_NS, name);
    for (const [key, value] of Object.entries(attributes)) {
      node.setAttribute(key, String(value));
    }
    return node;
  }

  function addTitle(node, text) {
    const title = el("title");
    title.textContent = text;
    node.appendChild(title);
    return node;
  }

  function edgeColor(edge) {
    return edge.polarity === "inhibitory" ? "#c9482b" : "#247a4a";
  }

  function nodeColor(node) {
    switch (node.kind) {
      case "sensory": return "#e1a72c";
      case "motor": return "#7c5bb7";
      case "readout": return "#2c6dba";
      default: return "#2f7d52";
    }
  }

  function formatNumber(value, digits = 2) {
    if (!Number.isFinite(value)) return "n/a";
    return value.toFixed(digits);
  }

  function drawLine(fragment, a, b, attributes) {
    const pa = project(a);
    const pb = project(b);
    const line = el("line", {
      x1: pa.x.toFixed(2),
      y1: pa.y.toFixed(2),
      x2: pb.x.toFixed(2),
      y2: pb.y.toFixed(2),
      ...attributes
    });
    fragment.appendChild(line);
    return (pa.z + pb.z) / 2;
  }

  function buildVolumeItems() {
    const items = [];
    const w = machine.widthMm;
    const d = machine.depthMm;
    const h = machine.heightMm;
    const corners = [
      { x: 0, y: 0, z: 0 }, { x: w, y: 0, z: 0 }, { x: w, y: d, z: 0 }, { x: 0, y: d, z: 0 },
      { x: 0, y: 0, z: h }, { x: w, y: 0, z: h }, { x: w, y: d, z: h }, { x: 0, y: d, z: h }
    ];
    const edges = [[0,1],[1,2],[2,3],[3,0],[4,5],[5,6],[6,7],[7,4],[0,4],[1,5],[2,6],[3,7]];
    for (const [a, b] of edges) {
      const fragment = document.createDocumentFragment();
      const z = drawLine(fragment, corners[a], corners[b], {
        stroke: "#768070",
        "stroke-width": 1.25,
        fill: "none"
      });
      items.push({ z, node: fragment });
    }

    const divisions = 8;
    for (let i = 1; i < divisions; i += 1) {
      const x = w * i / divisions;
      const y = d * i / divisions;
      let fragment = document.createDocumentFragment();
      let z = drawLine(fragment, { x, y: 0, z: 0 }, { x, y: d, z: 0 }, {
        stroke: "#c7d0c1",
        "stroke-width": 0.75,
        "stroke-dasharray": "4 4"
      });
      items.push({ z, node: fragment });
      fragment = document.createDocumentFragment();
      z = drawLine(fragment, { x: 0, y, z: 0 }, { x: w, y, z: 0 }, {
        stroke: "#c7d0c1",
        "stroke-width": 0.75,
        "stroke-dasharray": "4 4"
      });
      items.push({ z, node: fragment });
    }
    return items;
  }

  function edgeItems() {
    const scale = currentScale();
    return data.edges
      .filter((edge) => edge.path.length > 1)
      .map((edge) => {
        const projected = edge.path.map((tuple) => project(point(tuple)));
        const points = projected.map((p) => `${p.x.toFixed(2)},${p.y.toFixed(2)}`).join(" ");
        const depth = projected.reduce((sum, p) => sum + p.z, 0) / projected.length;
        const strokeWidth = clamp(edge.coreDiameterMm * scale * 0.42, 1.4, 9);
        const polyline = el("polyline", {
          points,
          fill: "none",
          stroke: edgeColor(edge),
          "stroke-width": strokeWidth.toFixed(2),
          "stroke-linecap": "round",
          "stroke-linejoin": "round",
          opacity: 0.86
        });
        if (edge.residualDelayUs > 0.001) {
          polyline.setAttribute("stroke-dasharray", "7 5");
        }
        addTitle(polyline, `${edge.id}: ${edge.sourceNode} -> ${edge.targetNode}; ${edge.polarity}; residual delay ${formatNumber(edge.residualDelayUs, 6)} us; transmission ${formatNumber(edge.estimatedTransmission, 4)}`);
        return { z: depth, node: polyline };
      });
  }

  function nodeItems() {
    const scale = currentScale();
    const shouldLabel = data.nodes.length <= 40;
    const items = [];
    for (const node of data.nodes) {
      const projected = project(point(node.position));
      const radius = clamp(node.radiusMm * scale * 0.72, 4, 22);
      const group = el("g", { opacity: 0.96 });
      const circle = el("circle", {
        cx: projected.x.toFixed(2),
        cy: projected.y.toFixed(2),
        r: radius.toFixed(2),
        fill: nodeColor(node),
        stroke: "#172016",
        "stroke-width": 1.1
      });
      addTitle(circle, `${node.id}: ${node.kind}; ${node.realisation}; threshold ${formatNumber(node.realisedThresholdMw, 6)} mW`);
      group.appendChild(circle);
      if (shouldLabel) {
        const text = el("text", {
          x: (projected.x + radius + 4).toFixed(2),
          y: (projected.y + 4).toFixed(2),
          fill: "#172016",
          "font-size": "11",
          "font-family": "Inter, Arial, sans-serif",
          "paint-order": "stroke",
          stroke: "#ffffff",
          "stroke-width": "3",
          "stroke-linejoin": "round"
        });
        text.textContent = node.id;
        group.appendChild(text);
      }
      items.push({ z: projected.z + radius * 0.02, node: group });
    }
    return items;
  }

  function hud() {
    const group = el("g", { "font-family": "Inter, Arial, sans-serif", "font-size": 13 });
    group.appendChild(el("rect", {
      x: 16,
      y: 16,
      width: 292,
      height: 86,
      rx: 7,
      fill: "#ffffff",
      stroke: "#c9d1c4",
      "stroke-width": 1
    }));
    const rows = [
      ["#247a4a", "excitatory waveguide"],
      ["#c9482b", "inhibitory waveguide"],
      ["#172016", "dashed route: non-geometric residual delay"]
    ];
    rows.forEach((row, index) => {
      const y = 40 + index * 22;
      group.appendChild(el("line", {
        x1: 30,
        y1: y - 4,
        x2: 62,
        y2: y - 4,
        stroke: row[0],
        "stroke-width": 4,
        "stroke-dasharray": index === 2 ? "7 5" : ""
      }));
      const text = el("text", { x: 72, y, fill: "#172016" });
      text.textContent = row[1];
      group.appendChild(text);
    });
    return group;
  }

  function render() {
    scene.replaceChildren();
    const fragment = document.createDocumentFragment();
    const items = [
      ...buildVolumeItems(),
      ...edgeItems(),
      ...nodeItems()
    ].sort((a, b) => a.z - b.z);
    for (const item of items) {
      fragment.appendChild(item.node);
    }
    fragment.appendChild(hud());
    scene.appendChild(fragment);
    status.textContent = `yaw ${state.yaw.toFixed(0)} deg, tilt ${state.pitch.toFixed(0)} deg, roll ${state.roll.toFixed(0)} deg, zoom ${state.zoom.toFixed(2)}x`;
  }

  function syncControls() {
    controls.yaw.value = String(Math.round(state.yaw));
    controls.pitch.value = String(Math.round(state.pitch));
    controls.roll.value = String(Math.round(state.roll));
    controls.zoom.value = String(Math.round(state.zoom * 100));
  }

  function setView(next) {
    Object.assign(state, next);
    state.pitch = clamp(state.pitch, -90, 90);
    state.zoom = clamp(state.zoom, 0.25, 8);
    syncControls();
    render();
  }

  controls.yaw.addEventListener("input", () => setView({ yaw: Number(controls.yaw.value) }));
  controls.pitch.addEventListener("input", () => setView({ pitch: Number(controls.pitch.value) }));
  controls.roll.addEventListener("input", () => setView({ roll: Number(controls.roll.value) }));
  controls.zoom.addEventListener("input", () => setView({ zoom: Number(controls.zoom.value) / 100 }));
  document.getElementById("view-reset").addEventListener("click", () => setView({ yaw: 35, pitch: 55, roll: 0, zoom: 1, panX: 0, panY: 0 }));
  document.getElementById("view-top").addEventListener("click", () => setView({ yaw: 0, pitch: 0, roll: 0, zoom: 1, panX: 0, panY: 0 }));
  document.getElementById("view-front").addEventListener("click", () => setView({ yaw: 0, pitch: 90, roll: 0, zoom: 1, panX: 0, panY: 0 }));
  document.getElementById("view-iso").addEventListener("click", () => setView({ yaw: 35, pitch: 55, roll: 0, zoom: 1, panX: 0, panY: 0 }));

  svg.addEventListener("wheel", (event) => {
    event.preventDefault();
    setView({ zoom: clamp(state.zoom * Math.exp(-event.deltaY * 0.001), 0.25, 8) });
  }, { passive: false });

  svg.addEventListener("pointerdown", (event) => {
    const rotateMode = event.shiftKey || event.button === 1 || event.button === 2;
    drag = {
      id: event.pointerId,
      x: event.clientX,
      y: event.clientY,
      mode: rotateMode ? "rotate" : "pan"
    };
    svg.classList.toggle("rotating", rotateMode);
    svg.setPointerCapture(event.pointerId);
  });

  svg.addEventListener("pointermove", (event) => {
    if (!drag || drag.id !== event.pointerId) return;
    const dx = event.clientX - drag.x;
    const dy = event.clientY - drag.y;
    drag.x = event.clientX;
    drag.y = event.clientY;
    if (drag.mode === "rotate" || event.shiftKey) {
      setView({
        yaw: state.yaw + dx * 0.35,
        pitch: clamp(state.pitch - dy * 0.35, -90, 90)
      });
    } else {
      setView({
        panX: state.panX + dx,
        panY: state.panY + dy
      });
    }
  });

  function endDrag(event) {
    if (drag && drag.id === event.pointerId) {
      drag = null;
      svg.classList.remove("rotating");
    }
  }

  svg.addEventListener("pointerup", endDrag);
  svg.addEventListener("pointercancel", endDrag);
  svg.addEventListener("contextmenu", (event) => event.preventDefault());

  syncControls();
  render();
})();
  </script>
</main>
</body>
</html>"##,
    );
    html
}

fn preview_data_json(plan: &PhysicalPlan, machine: &MachineProfile) -> String {
    let nodes = plan
        .nodes
        .iter()
        .map(|node| {
            json!({
                "id": &node.id,
                "kind": node_kind_label(&node.logical_kind),
                "position": [
                    node.position_mm.x,
                    node.position_mm.y,
                    node.position_mm.z
                ],
                "radiusMm": node.radius_mm,
                "realisation": node_realisation_label(&node.realisation),
                "logicalThreshold": node.logical_threshold,
                "realisedThresholdMw": node.realised_threshold_mw,
                "logicalRefractoryUs": node.logical_refractory_us,
                "realisedRefractoryUs": node.realised_refractory_us,
                "materialId": &node.material_id,
                "sourceComponentIds": &node.source_component_ids
            })
        })
        .collect::<Vec<_>>();
    let edges = plan
        .connections
        .iter()
        .map(|edge| {
            let path = edge
                .path_mm
                .iter()
                .map(|point| vec![point.x, point.y, point.z])
                .collect::<Vec<_>>();
            json!({
                "id": &edge.id,
                "sourceNode": &edge.source_node,
                "targetNode": &edge.target_node,
                "polarity": polarity_label(&edge.polarity),
                "path": path,
                "pathLengthMm": edge.path_length_mm,
                "coreMaterialId": &edge.core_material_id,
                "claddingMaterialId": &edge.cladding_material_id,
                "coreDiameterMm": edge.core_diameter_mm,
                "targetDelayUs": edge.target_delay_us,
                "geometricDelayUs": edge.geometric_delay_us,
                "residualDelayUs": edge.residual_delay_us,
                "targetTransmission": edge.target_transmission,
                "estimatedTransmission": edge.estimated_transmission,
                "implementation": edge_implementation_label(&edge.implementation),
                "sourceComponentIds": &edge.source_component_ids
            })
        })
        .collect::<Vec<_>>();
    let data = json!({
        "sourceNetworkId": &plan.source_network_id,
        "sourceTick": plan.source_tick,
        "machine": {
            "id": &machine.id,
            "model": &machine.model,
            "widthMm": machine.build_volume_mm.x.max(1.0),
            "depthMm": machine.build_volume_mm.y.max(1.0),
            "heightMm": machine.build_volume_mm.z.max(1.0)
        },
        "nodes": nodes,
        "edges": edges
    });
    serde_json::to_string(&data).unwrap_or_else(|_| "{}".to_string())
}

fn node_kind_label(kind: &NodeKind) -> &'static str {
    match kind {
        NodeKind::Sensory => "sensory",
        NodeKind::Interneuron => "interneuron",
        NodeKind::Motor => "motor",
        NodeKind::Readout => "readout",
    }
}

fn node_realisation_label(realisation: &NodeRealisation) -> &'static str {
    match realisation {
        NodeRealisation::PassiveFluorescent => "passive_fluorescent",
        NodeRealisation::SoftThresholdFluorescent => "soft_threshold_fluorescent",
        NodeRealisation::Photochromic => "photochromic",
        NodeRealisation::Thermoresponsive => "thermoresponsive",
        NodeRealisation::OptoelectronicRegenerator => "optoelectronic_regenerator",
    }
}

fn polarity_label(polarity: &Polarity) -> &'static str {
    match polarity {
        Polarity::Excitatory => "excitatory",
        Polarity::Inhibitory => "inhibitory",
    }
}

fn edge_implementation_label(implementation: &EdgeImplementation) -> &'static str {
    match implementation {
        EdgeImplementation::DirectWaveguide => "direct_waveguide",
        EdgeImplementation::AttenuatedWaveguide => "attenuated_waveguide",
        EdgeImplementation::FluorescentDelayNode => "fluorescent_delay_node",
        EdgeImplementation::ExternalDelayChannel => "external_delay_channel",
        EdgeImplementation::DualRailInhibitory => "dual_rail_inhibitory",
    }
}

fn min_z(path: &[Vec3]) -> f64 {
    path.iter().map(|p| p.z).fold(f64::INFINITY, f64::min)
}

fn max_z(path: &[Vec3]) -> f64 {
    path.iter().map(|p| p.z).fold(f64::NEG_INFINITY, f64::max)
}

fn xml_escape(value: &str) -> String {
    value
        .replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
}

fn html_escape(value: &str) -> String {
    xml_escape(value)
}

fn html_script_json(value: &str) -> String {
    value
        .replace('&', "\\u0026")
        .replace('<', "\\u003c")
        .replace('>', "\\u003e")
}
