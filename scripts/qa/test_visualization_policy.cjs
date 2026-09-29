"use strict";

const assert = require("node:assert/strict");
const fs = require("node:fs");
const path = require("node:path");

require(path.join(__dirname, "../../web_ui/visualization-policy.js"));
const policy = globalThis.AARNNVisualizationPolicy;
const fixture = JSON.parse(fs.readFileSync(
  path.join(__dirname, "../../qa/fixtures/visualization/complexity-policy-v1.json"),
  "utf8",
));

assert.equal(policy.version, fixture.version);
assert.deepEqual(policy.stages, fixture.stages);
assert.deepEqual(policy.labels, fixture.labels);
assert.equal(policy.p95Milliseconds(fixture.p95_sample_ms), fixture.expected_p95_ms);
assert.equal(policy.p95Milliseconds([1, Number.NaN, -1, Number.POSITIVE_INFINITY]), 1);
for (const [filePath, expression] of [
  ["../../apps/android/app/src/main/java/com/neuralmimicry/aarnn/VisualizationPolicy.kt", /val stages = listOf\(([\s\S]*?)\n    \)/],
  ["../../apps/ios/VisualizationPolicy.swift", /static let stages = \[([\s\S]*?)\n    \]/],
]) {
  const source = fs.readFileSync(path.join(__dirname, filePath), "utf8");
  const block = source.match(expression);
  assert.ok(block, `could not find shared stage labels in ${filePath}`);
  assert.deepEqual(Array.from(block[1].matchAll(/"([^"]+)"/g), match => match[1]), fixture.labels);
}
for (const [index, boundary] of fixture.zoom_upper_bounds.entries()) {
  assert.equal(policy.zoomStage(boundary - Number.EPSILON * boundary), index + 1);
}
for (const [index, boundary] of fixture.latency_upper_bounds_ms.entries()) {
  assert.equal(policy.latencyStage(boundary), 9 - index);
}
assert.equal(policy.latencyStage(81), 1);
assert.equal(policy.latencyStage(Number.NaN), 1);
assert.equal(policy.update(8, 4, 4, 6, 0), 6);
assert.equal(policy.update(5, 4, 4, 9, 1499), 5);
assert.equal(policy.update(5, 4, 4, 9, 1500), 6);

const anatomical = {
  nodes: [{ soma_radius_mm: 0.04 }],
  edges: [{ source: {}, target: {} }],
  paths: [{ radius_mm: 0.006 }],
  markers: [{ kind: "synapse" }],
  coverage: { volumetric_clearance_verified: true },
};
assert.equal(policy.highestSupported({ nodes: [{}] }, anatomical), 9);
assert.equal(policy.highestSupported({ nodes: [{}] }, {
  ...anatomical,
  nodes: [{ }],
}), 6, "missing physical soma radius must suppress clearance-dependent stages");
assert.equal(policy.highestSupported(null, {
  ...anatomical,
  coverage: { volumetric_clearance_verified: false },
}), 6, "missing 3D clearance evidence must suppress physical stages");
assert.equal(policy.highestSupported(null, {
  ...anatomical,
  nodes: [{ }],
}), 6, "missing physical soma radius must suppress clearance-dependent stages");

console.log("visualisation policy fixture and stage safety checks passed");
