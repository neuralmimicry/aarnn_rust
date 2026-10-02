/* Shared browser-side stage policy; thresholds match the versioned QA fixture. */
(function (root) {
  "use strict";
  const STAGES = Object.freeze([
    "synthetic_pixels", "synthetic_connections", "synthetic_neurons",
    "anatomical_pixels", "anatomical_straight_edges", "anatomical_branching_edges",
    "anatomical_centre_lines", "anatomical_volumes", "anatomical_contacts"
  ]);
  const LABELS = Object.freeze([
    "Synthetic pixels", "Synthetic connections", "Synthetic neurons",
    "Anatomical pixels", "Straight connections", "Branching connections",
    "Anatomical centre-lines", "Anatomical volumes", "Synapses and boutons"
  ]);
  const ZOOM_UPPER_BOUNDS = Object.freeze([0.45, 0.65, 0.9, 1.2, 1.55, 1.95, 2.5, 3.2]);
  const LATENCY_UPPER_BOUNDS_MS = Object.freeze([5, 8, 12, 16, 24, 33, 50, 80]);
  const UPSHIFT_DWELL_MS = 1500;

  function zoomStage(zoom) {
    const value = Number(zoom);
    if (!Number.isFinite(value)) return 1;
    return ZOOM_UPPER_BOUNDS.findIndex(bound => value < bound) + 1 || STAGES.length;
  }

  function latencyStage(latencyMs) {
    const value = Number(latencyMs);
    if (!Number.isFinite(value)) return 1;
    for (let index = 0; index < LATENCY_UPPER_BOUNDS_MS.length; index += 1) {
      if (value <= LATENCY_UPPER_BOUNDS_MS[index]) return 9 - index;
    }
    return 1;
  }

  function p95Milliseconds(samples) {
    if (!Array.isArray(samples)) return 16;
    const ordered = samples.filter(value => Number.isFinite(value) && value >= 0)
      .slice().sort((left, right) => left - right);
    if (!ordered.length) return 16;
    return ordered[Math.floor((ordered.length - 1) * 0.95)];
  }

  function update(current, zoom, latencyMs, highestAvailable, dwellMs) {
    const currentStage = Math.max(1, Math.min(9, Math.trunc(Number(current) || 1)));
    const highest = Math.max(1, Math.min(9, Math.trunc(Number(highestAvailable) || 1)));
    const target = Math.min(zoomStage(zoom), latencyStage(latencyMs), highest);
    if (target < currentStage) return target;
    if (target > currentStage && Number(dwellMs) >= UPSHIFT_DWELL_MS) return currentStage + 1;
    return Math.min(currentStage, highest);
  }

  function highestSupported(synthetic, anatomical) {
    const hasSynthetic = Boolean(synthetic && Array.isArray(synthetic.nodes) && synthetic.nodes.length);
    if (!anatomical || !Array.isArray(anatomical.nodes)) {
      return hasSynthetic ? 3 : 1;
    }
    const coverage = anatomical.coverage || {};
    if (!anatomical.nodes.length) {
      return coverage.complete === true && coverage.volumetric_clearance_verified === true &&
        coverage.contact_set_verified === true ? 9 : (hasSynthetic ? 3 : 1);
    }
    let highest = 4;
    if (Array.isArray(anatomical.edges) && anatomical.edges.length) highest = 6;
    if (coverage.volumetric_clearance_verified === true && Array.isArray(anatomical.paths) &&
        (anatomical.paths.length || coverage.contact_set_verified === true)) {
      const physicalSomas = anatomical.nodes.every(node => Number.isFinite(Number(node.soma_radius_mm)) && Number(node.soma_radius_mm) > 0);
      const physicalNeurites = anatomical.paths.every(path => Number.isFinite(Number(path.radius_mm)) && Number(path.radius_mm) > 0);
      if (physicalSomas && physicalNeurites) {
        // Clearance is meaningful only when the published physical radii are
        // present as well as the producer's collision-check witness.
        highest = 8;
        if (coverage.complete === true && coverage.contact_set_verified === true) highest = 9;
      }
    }
    return highest;
  }

  root.AARNNVisualizationPolicy = Object.freeze({
    version: 1,
    stages: STAGES,
    labels: LABELS,
    zoomStage,
    latencyStage,
    p95Milliseconds,
    update,
    highestSupported,
    upshiftDwellMs: UPSHIFT_DWELL_MS
  });
}(typeof globalThis === "undefined" ? this : globalThis));
