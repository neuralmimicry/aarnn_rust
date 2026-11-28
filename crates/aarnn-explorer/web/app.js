// SPDX-FileCopyrightText: masterkiga
// SPDX-License-Identifier: MIT
//
// AARNN connectome explorer: browser presentation layer. Rust owns every
// analysis, experiment and verdict; this file fetches bounded views, draws
// them and submits jobs. Neuron and synapse IDs are always strings, and
// plans only ever carry stable IDs (never display indices).
"use strict";

const API = "/api/explorer/v1";
const CSRF = document.querySelector('meta[name="explorer-csrf"]')?.content || "";
const TERMINAL = new Set(["succeeded", "failed", "cancelled", "interrupted"]);
const S = {
  source: null, sources: [], index: null, topo: new Map(), objects: new Map(), jobs: new Map(),
  basket: { neurons: [], synapses: [] }, history: [], plan: [], planHistory: [],
  highlight: null, selected: null, active: null, built: new Set(), hosts: { basket: [], hl: [] },
};
let uid = 0;

// ---------------------------------------------------------------- DOM helpers
const PROPS = new Set(["value", "checked", "selected", "disabled", "hidden", "open"]);
function h(tag, props, ...kids) {
  const [name, ...cls] = tag.split(".");
  const e = document.createElement(name);
  if (cls.length) e.className = cls.join(" ");
  for (const [k, v] of Object.entries(props || {})) {
    if (v == null || v === false) continue;
    if (k === "text") e.textContent = String(v);
    else if (k === "class") e.classList.add(...String(v).split(" ").filter(Boolean));
    else if (k.startsWith("on") && typeof v === "function") e.addEventListener(k.slice(2), v);
    else if (PROPS.has(k)) e[k] = v;
    else e.setAttribute(k, v === true ? "" : String(v));
  }
  for (const c of kids.flat(Infinity)) if (c != null && c !== false) e.append(c instanceof Node ? c : String(c));
  return e;
}
const $ = (sel) => document.querySelector(sel);
const enc = encodeURIComponent;
const fmt = (x) => x == null ? "–" : typeof x !== "number" ? String(x) : Number.isInteger(x) ? x.toLocaleString("en-GB") : Number(x.toPrecision(5)).toLocaleString("en-GB", { maximumFractionDigits: 6 });
const fmtBytes = (n) => n == null ? "–" : `${fmt(n)} B` + (n >= 1024 ? ` (${(n / 1024).toFixed(1)} KiB)` : "");
const fmtNs = (n) => n == null ? "–" : `${(n / 1e6).toFixed(3)} ms`;
const cell = (v) => v instanceof Node ? v : v == null ? "–" : typeof v === "boolean" ? (v ? "yes" : "no") : typeof v === "number" ? fmt(v) : Array.isArray(v) ? (v.length ? v.join(", ") : "none") : typeof v === "object" ? JSON.stringify(v) : String(v);
const code = (t) => h("code.hash", { text: t ?? "–" });
const note = (t, cls = "note") => h("p." + cls, { text: t });
const errBox = (e) => h("p.error", { role: "alert", text: String((e && e.message) || e) });
const natural = (a, b) => String(a).localeCompare(String(b), "en-GB", { numeric: true });

function verdictClass(s) {
  s = String(s);
  if (/^(passed|succeeded|complete|enforced|validated|yes|present|calibrated|candidate|available)\b/.test(s)) return "ok";
  if (/^(failed|error|validation-failed|negative|no\b|absent|unavailable|missing|incomplete|unknown)/.test(s)) return "bad";
  if (/^(inconclusive|cancelled|pausing|paused|running|queued|advisory|estimated|exploratory|interrupted|truncated|approximate|excluded|unsupported|open)/.test(s)) return "warn";
  return "na";
}
function badge(v) {
  const s = v === true ? "passed" : v === false ? "failed" : v == null ? "not assessed" : String(v);
  return h("span.badge." + verdictClass(s), { text: s });
}
const yesno = (v, yes, no) => h("span.badge." + (v ? "warn" : "ok"), { text: v ? yes : no });

function table(caption, cols, rows, o = {}) {
  const max = o.max ?? 500, shown = rows.slice(0, max);
  const t = h("table", null, h("caption", { text: caption }),
    h("thead", null, h("tr", null, cols.map((c) => h("th", { scope: "col", text: c[0] })))),
    h("tbody", null, shown.map((r, i) => h("tr", { class: o.rowClass ? o.rowClass(r) : null }, cols.map((c) => h("td", null, cell(c[1](r, i))))))));
  return h("div.table-wrap", { tabindex: 0, role: "region", "aria-label": caption }, t,
    rows.length > max ? note(`Display truncation: showing ${max} of ${rows.length} rows.`) : null,
    rows.length ? null : note(o.empty || "No rows."));
}
function dl(pairs) {
  return h("dl.kv", null, pairs.filter(Boolean).map(([k, v]) => [h("dt", { text: k }), h("dd", null, cell(v))]));
}
const ul = (items, empty = "none") => items && items.length ? h("ul.plain", null, items.map((x) => h("li", null, cell(x)))) : note(empty);
function field(label, input, hint) {
  input.id = input.id || `f${++uid}`;
  const hn = hint ? h("span.hint", { id: `${input.id}-hint`, text: hint }) : null;
  if (hn) input.setAttribute("aria-describedby", hn.id);
  return h("div.field", null, h("label", { for: input.id, text: label }), input, hn);
}
function selectEl(options, value, onchange) {
  const s = h("select", { onchange: onchange ? () => onchange(s.value) : null },
    options.map(([v, t]) => h("option", { value: v, text: t, selected: v === value })));
  return s;
}
const button = (text, onclick, o = {}) => h("button", { type: "button", text, onclick, class: o.cls, disabled: o.disabled, "aria-pressed": o.pressed });
const numInput = (value, o = {}) => h("input", { type: "number", value: String(value), step: o.step ?? "any", min: o.min, max: o.max, inputmode: "decimal" });
const textInput = (value, o = {}) => h("input", { type: o.type || "text", value: value ?? "", placeholder: o.placeholder, spellcheck: "false", autocomplete: "off" });

function announce(text) {
  const a = $("#announce");
  a.textContent = "";
  setTimeout(() => { a.textContent = text; }, 30);
}

// ---------------------------------------------------------------- API
async function api(path, body) {
  const init = { credentials: "same-origin", headers: { accept: "application/json" } };
  if (body !== undefined) Object.assign(init, { method: "POST", body: JSON.stringify(body), headers: { "content-type": "application/json", "x-explorer-csrf": CSRF } });
  const r = await fetch(API + path, init);
  const text = await r.text();
  let j = null;
  try { j = JSON.parse(text); } catch (_) { j = null; }
  if (!r.ok) throw new Error(`${path.split("?")[0]}: HTTP ${r.status}: ${j ? j.message || j.error || text : text || r.statusText}`);
  return j;
}
// Objects are content-addressed, so a cached copy never goes stale.
function object(hash) {
  if (!S.objects.has(hash)) S.objects.set(hash, api(`/object/${enc(hash)}`).catch((e) => { S.objects.delete(hash); throw e; }));
  return S.objects.get(hash);
}
async function loadIndex() { S.index = await api("/index"); return S.index; }
const entries = (kind) => Object.entries((S.index && S.index[kind]) || {}).sort((a, b) => natural(a[0], b[0]));

function topoCache(src) {
  if (!S.topo.has(src)) S.topo.set(src, { byIndex: new Map(), byId: new Map(), syn: new Map(), pages: new Map(), meta: null });
  return S.topo.get(src);
}
async function topoPage(src, offset, limit) {
  const t = topoCache(src), key = `${offset}:${limit}`;
  if (!t.pages.has(key)) {
    const p = await api(`/topology?source=${enc(src)}&offset=${offset}&limit=${limit}`);
    t.meta = { total_neurons: p.total_neurons, total_synapses: p.total_synapses, capability: p.capability, assumptions: p.assumptions, limitations: p.limitations, display_note: p.display_note, max_limit: p.limit };
    for (const n of p.neurons) { t.byIndex.set(n.index, n); t.byId.set(n.id, n); }
    for (const s of p.synapses) t.syn.set(s.id, s);
    t.pages.set(key, p);
  }
  return t.pages.get(key);
}
async function ensureIndices(src, idxs) {
  const t = topoCache(src), need = new Set();
  for (const i of idxs) if (!t.byIndex.has(i)) need.add(Math.floor(i / 2000));
  for (const pg of need) await topoPage(src, pg * 2000, 2000);
  return t;
}
const neuronName = (src, i) => topoCache(src).byIndex.get(i)?.id ?? `#${i}`;

// ---------------------------------------------------------------- jobs
async function submitJob(body, label, onDone) {
  const r = await api("/jobs", body);
  announce(`${label}: submitted as ${r.job}${r.duplicate ? " (identical job already known; reusing it)" : ""}`);
  return trackJob(r.job, body.kind, label, body, onDone);
}
function trackJob(id, kind, label, params, onDone) {
  let j = S.jobs.get(id);
  if (j && !TERMINAL.has(j.state)) { if (onDone) j.cbs.push(onDone); return j; }
  if (!j) {
    j = { id, kind, label, params, cbs: [] };
    S.jobs.set(id, j);
    jobNode(j);
  } else $("#job-list").prepend(j.node);
  Object.assign(j, { state: "queued", cursor: 0, events: [], started: Date.now(), ended: null, error: null, result: null, polls: 0, lastPollError: null });
  if (onDone) j.cbs = [onDone];
  updateJob(j);
  poll(j);
  return j;
}
async function poll(j) {
  try {
    const r = await api(`/jobs/${enc(j.id)}/events?cursor=${j.cursor}`);
    j.events.push(...r.events);
    j.cursor = r.next_cursor;
    j.lastPollError = null;
    if (r.state !== j.state) { j.state = r.state; announce(`${j.label}: ${r.state}`); }
    if (TERMINAL.has(r.state)) {
      const full = await api(`/jobs/${enc(j.id)}`);
      Object.assign(j, { state: full.state, result: full.result, error: full.error, ended: Date.now() });
      updateJob(j);
      announce(`${j.label}: ${j.state}${j.error ? ` (${j.error})` : ""}`);
      if (j.state === "succeeded") { await loadIndex().catch(() => null); staleOthers(); }
      for (const cb of j.cbs) { try { cb(j); } catch (e) { console.error(e); } }
      return;
    }
  } catch (e) {
    j.lastPollError = e.message;
    if (++j.polls > 60) { j.state = "unknown"; j.error = `lost contact: ${e.message}`; updateJob(j); return; }
  }
  updateJob(j);
  setTimeout(() => poll(j), 1000);
}
function jobNode(j) {
  const u = {
    st: h("span.badge"), tm: h("span.muted"), msg: h("span.jobmsg"),
    pr: h("progress", { "aria-label": `${j.label} in progress` }),
    cancel: button("Cancel", () => cancelJob(j, false)),
    pause: j.kind === "optimise" ? button("Pause", () => cancelJob(j, true)) : null,
    resume: j.kind === "optimise" ? button("Resume", () => submitJob(j.params, j.label, j.cbs[0]).catch((e) => announce(e.message))) : null,
    dismiss: button("Dismiss", () => { j.node.remove(); $("#jobs-heading").focus(); }),
  };
  j.ui = u;
  j.node = h("li.job", null, h("strong", { text: j.label }), " ", h("code", { text: j.id }), " ", u.st, " ", u.tm, " ", u.pr, u.msg, h("span.jobbtns", null, u.cancel, u.pause, u.resume, u.dismiss));
  $("#job-list").prepend(j.node);
}
function updateJob(j) {
  const u = j.ui, done = TERMINAL.has(j.state) || j.state === "unknown";
  u.st.textContent = j.paused && j.state === "cancelled" ? "paused (cancelled; completed work is kept)" : j.state;
  u.st.className = "badge " + verdictClass(j.state);
  u.tm.textContent = `${Math.round(((j.ended || Date.now()) - j.started) / 1000)} s`;
  u.pr.hidden = done;
  u.cancel.hidden = done;
  if (u.pause) u.pause.hidden = done;
  if (u.resume) u.resume.hidden = !(done && j.state === "cancelled");
  u.dismiss.hidden = !done;
  u.msg.textContent = j.error ? ` ${j.error}` : j.lastPollError ? ` (retrying: ${j.lastPollError})` : j.state === "pausing" ? " stopping admission; running workers are terminated" : "";
}
async function cancelJob(j, pause) {
  try {
    j.paused = pause;
    const r = await api(`/jobs/${enc(j.id)}/cancel`, {});
    j.state = r.state;
    updateJob(j);
    announce(`${j.label}: ${pause ? "pause" : "cancellation"} requested (${r.state})`);
  } catch (e) { announce(e.message); }
}
function staleOthers() { for (const v of [...S.built]) if (v !== S.active) S.built.delete(v); }

// ---------------------------------------------------------------- canvas helpers
function prep(cv, cssH) {
  const dpr = window.devicePixelRatio || 1, w = Math.max(240, cv.clientWidth || cv.parentElement?.clientWidth || 640);
  cv.width = Math.round(w * dpr);
  cv.height = Math.round(cssH * dpr);
  const ctx = cv.getContext("2d");
  ctx.setTransform(dpr, 0, 0, dpr, 0, 0);
  ctx.clearRect(0, 0, w, cssH);
  return { ctx, w, h: cssH };
}
function colours() {
  const cs = getComputedStyle(document.documentElement), g = (n) => cs.getPropertyValue(`--${n}`).trim();
  return Object.fromEntries(["fg", "muted", "panel", "grid", "exc", "inh", "gap", "sens", "hid", "out", "hi", "accent", "bad", "ok", "warn"].map((k) => [k, g(k)]));
}
function niceTicks(a, b, n = 6) {
  const span = b - a || 1, raw = span / n, mag = 10 ** Math.floor(Math.log10(raw));
  const step = [1, 2, 5, 10].map((m) => m * mag).find((s) => span / s <= n) || raw;
  const out = [];
  for (let v = Math.ceil(a / step) * step; v <= b + step * 1e-9; v += step) out.push(+v.toPrecision(12));
  return out;
}
function zoomAt(v, x, y, f) {
  const ns = Math.max(0.02, Math.min(60, v.s * f));
  f = ns / v.s;
  v.tx = x - (x - v.tx) * f;
  v.ty = y - (y - v.ty) * f;
  v.s = ns;
}
function panZoom(cv, v, draw, onClick) {
  let drag = null;
  cv.addEventListener("wheel", (e) => {
    e.preventDefault();
    const r = cv.getBoundingClientRect();
    zoomAt(v, e.clientX - r.left, e.clientY - r.top, e.deltaY < 0 ? 1.15 : 1 / 1.15);
    draw();
  }, { passive: false });
  cv.addEventListener("pointerdown", (e) => { drag = { x: e.clientX, y: e.clientY, tx: v.tx, ty: v.ty, moved: false }; cv.setPointerCapture(e.pointerId); });
  cv.addEventListener("pointermove", (e) => {
    if (!drag) return;
    const dx = e.clientX - drag.x, dy = e.clientY - drag.y;
    if (Math.abs(dx) + Math.abs(dy) > 4) drag.moved = true;
    if (drag.moved) { v.tx = drag.tx + dx; v.ty = drag.ty + dy; draw(); }
  });
  cv.addEventListener("pointerup", (e) => {
    const d = drag;
    drag = null;
    if (d && !d.moved) { const r = cv.getBoundingClientRect(); onClick(e.clientX - r.left, e.clientY - r.top, e); }
  });
  cv.addEventListener("keydown", (e) => {
    const k = e.key, c = [cv.clientWidth / 2, cv.clientHeight / 2];
    const acts = { ArrowLeft: () => { v.tx += 40; }, ArrowRight: () => { v.tx -= 40; }, ArrowUp: () => { v.ty += 40; }, ArrowDown: () => { v.ty -= 40; },
      "+": () => zoomAt(v, ...c, 1.25), "=": () => zoomAt(v, ...c, 1.25), "-": () => zoomAt(v, ...c, 0.8), 0: () => { v.fitted = false; } };
    if (acts[k]) { e.preventDefault(); acts[k](); draw(); }
  });
}
function axes(ctx, C, box, xr, yr, xl, yl) {
  ctx.strokeStyle = C.grid; ctx.fillStyle = C.muted; ctx.lineWidth = 1; ctx.font = "11px system-ui, sans-serif";
  const X = (x) => box.l + ((x - xr[0]) / (xr[1] - xr[0] || 1)) * (box.r - box.l);
  const Y = (y) => box.b - ((y - yr[0]) / (yr[1] - yr[0] || 1)) * (box.b - box.t);
  ctx.textAlign = "center";
  for (const t of niceTicks(xr[0], xr[1])) { ctx.beginPath(); ctx.moveTo(X(t), box.t); ctx.lineTo(X(t), box.b); ctx.stroke(); ctx.fillText(fmt(t), X(t), box.b + 14); }
  ctx.textAlign = "right";
  if (yr) for (const t of niceTicks(yr[0], yr[1], 5)) { ctx.beginPath(); ctx.moveTo(box.l, Y(t)); ctx.lineTo(box.r, Y(t)); ctx.stroke(); ctx.fillText(fmt(t), box.l - 4, Y(t) + 4); }
  ctx.fillStyle = C.fg; ctx.textAlign = "center";
  if (xl) ctx.fillText(xl, (box.l + box.r) / 2, box.b + 28);
  if (yl) { ctx.save(); ctx.translate(12, (box.t + box.b) / 2); ctx.rotate(-Math.PI / 2); ctx.fillText(yl, 0, 0); ctx.restore(); }
  ctx.textAlign = "left";
  return { X, Y };
}
function marker(ctx, shape, x, y, r) {
  ctx.beginPath();
  if (shape === "cross") { ctx.moveTo(x - r, y - r); ctx.lineTo(x + r, y + r); ctx.moveTo(x + r, y - r); ctx.lineTo(x - r, y + r); ctx.stroke(); return; }
  if (shape === "diamond") { ctx.moveTo(x, y - r - 1); ctx.lineTo(x + r + 1, y); ctx.lineTo(x, y + r + 1); ctx.lineTo(x - r - 1, y); ctx.closePath(); ctx.fill(); ctx.stroke(); return; }
  ctx.arc(x, y, r, 0, Math.PI * 2);
  if (shape === "dot") ctx.fill();
  ctx.stroke();
}
function legend(items) {
  return h("ul.legend", { "aria-label": "Legend" }, items.map(([cls, text]) => h("li", null, h("span.sw." + cls, { "aria-hidden": "true" }), text)));
}

// ---------------------------------------------------------------- shared panels
const CLAIMS = [
  ["behavioural_preservation", "Behavioural preservation", "Named tasks pass over the declared inputs, seeds and environment."],
  ["reference_agreement", "Reference agreement", "The candidate matches the intact computational reference within declared tolerances."],
  ["resilience", "Resilience", "A named fault family, magnitude and evidence procedure pass."],
  ["biological_validity", "Biological validity", "Only named experimental observations count; untested biology stays unvalidated."],
  ["hardware_benefit", "Hardware benefit", "A measured target improvement, or a clearly labelled estimate."],
];
function claimsPanel(verdicts, context) {
  return h("section.claims", { "aria-label": "Claims, each judged separately" },
    h("h3", { text: "Claims (five separate kinds)" }),
    h("ul.claim-grid", null, CLAIMS.map(([k, t, d]) => h("li.claim", null, h("h4", { text: t }), badge(verdicts ? verdicts[k] : null), h("p.hint", { text: d })))),
    verdicts && verdicts.implementation ? note(`Implementation correctness (separate again): ${verdicts.implementation}`) : null,
    context ? note(context) : null);
}
function capabilityBox(cap) {
  const defs = [["exact-resume", "Continues an exact running state."], ["declared-cold-start", "Executable from a declared initial state at tick 0; no in-flight events."], ["structural-only", "Anatomy only: no dynamics, so no activity, GNATs or functional tests."]];
  return h("dl.kv.caps", null, defs.map(([k, d]) => [h("dt", null, k === cap ? badge(`${k} (this source)`) : k), h("dd", { text: d })]));
}
function basketBox() {
  const box = h("section.basket", { "aria-label": "Intervention selection" });
  S.hosts.basket.push(box);
  fillBasket(box);
  return box;
}
function fillBasket(box) {
  const had = box.contains(document.activeElement), b = S.basket;
  const chip = (id, kind) => h("li", null, h("code", { text: id }), " ", button("Remove", () => changeBasket((x) => { x[kind] = x[kind].filter((y) => y !== id); }, `Removed ${id}`), { cls: "small" }));
  box.replaceChildren(...[
    h("h3", { tabindex: -1, text: `Intervention selection: ${b.neurons.length} neurons, ${b.synapses.length} synapses` }),
    note("Uncommitted stable IDs. Nothing runs until a plan is submitted in the Intervention view."),
    b.neurons.length ? h("ul.chips", { "aria-label": "Selected neurons" }, b.neurons.map((id) => chip(id, "neurons"))) : null,
    b.synapses.length ? h("ul.chips", { "aria-label": "Selected synapses" }, b.synapses.map((id) => chip(id, "synapses"))) : null,
    h("div.row", null, button("Undo last selection change", undoBasket, { disabled: !S.history.length }), button("Clear selection", () => changeBasket((x) => { x.neurons = []; x.synapses = []; }, "Selection cleared"), { disabled: !b.neurons.length && !b.synapses.length }))].filter(Boolean));
  if (had) box.querySelector("h3").focus();
}
function changeBasket(mut, msg) {
  S.history.push(JSON.stringify(S.basket));
  if (S.history.length > 200) S.history.shift();
  mut(S.basket);
  S.basket.neurons = [...new Set(S.basket.neurons)];
  S.basket.synapses = [...new Set(S.basket.synapses)];
  refreshBasket();
  if (msg) announce(msg);
}
function undoBasket() {
  const p = S.history.pop();
  if (p) { S.basket = JSON.parse(p); refreshBasket(); announce("Last selection change undone"); }
}
function refreshBasket() {
  S.hosts.basket = S.hosts.basket.filter((b) => b.isConnected);
  S.hosts.basket.forEach(fillBasket);
  for (const t of document.querySelectorAll("button.tog")) {
    const on = S.basket[t.dataset.kind].includes(t.dataset.sid);
    t.setAttribute("aria-pressed", String(on));
    t.textContent = on ? "In selection" : "Add";
  }
  drawConnectome();
}
const togButton = (id, kind) => {
  const on = S.basket[kind].includes(id);
  const b = button(on ? "In selection" : "Add", () => changeBasket((x) => { x[kind] = x[kind].includes(id) ? x[kind].filter((y) => y !== id) : [...x[kind], id]; }, `${id} ${S.basket[kind].includes(id) ? "removed from" : "added to"} the selection`), { cls: "tog small", pressed: String(on) });
  b.dataset.sid = id;
  b.dataset.kind = kind;
  b.setAttribute("aria-label", `Toggle ${id} in the intervention selection`);
  return b;
};
function hlBanner() {
  const box = h("div.hlbanner", { "aria-live": "polite" });
  S.hosts.hl.push(box);
  fillHl(box);
  return box;
}
function fillHl(box) {
  const x = S.highlight;
  box.replaceChildren(...(x ? [h("span", { text: `Highlighted ${x.kind} ${x.id} (task ${x.task}): ${x.neurons.length} neurons` }), " ", button("Clear highlight", () => setHighlight(null), { cls: "small" })] : []));
  box.hidden = !x;
}
function setHighlight(x) {
  S.highlight = x;
  S.hosts.hl = S.hosts.hl.filter((b) => b.isConnected);
  S.hosts.hl.forEach(fillHl);
  announce(x ? `Highlighted ${x.kind} ${x.id}: its ${x.neurons.length} neurons are marked in the connectome and activity views` : "Highlight cleared");
  drawConnectome();
  drawRaster();
  drawBars();
}

// ---------------------------------------------------------------- views
const VIEWS = {};
function view(name, render, onShow) { VIEWS[name] = { render, onShow }; }
async function build(name, focus) {
  const root = $(`#v-${name}`);
  root.replaceChildren(note("Loading…", "loading"));
  S.built.add(name);
  try { await VIEWS[name].render(root); } catch (e) { root.append(errBox(e)); }
  if (focus) root.querySelector("h2")?.focus();
}
function showTab(name, focus) {
  if (!VIEWS[name]) name = "project";
  S.active = name;
  for (const t of document.querySelectorAll('[role="tab"]')) {
    const on = t.dataset.view === name;
    t.setAttribute("aria-selected", String(on));
    t.tabIndex = on ? 0 : -1;
    if (on && focus) t.focus();
    $(`#${t.getAttribute("aria-controls")}`).hidden = !on;
  }
  if (location.hash !== `#${name}`) history.replaceState(null, "", `#${name}`);
  if (!S.built.has(name)) build(name);
  else VIEWS[name].onShow?.();
}
const heading = (text, name) => h("div.vhead", null, h("h2", { text, tabindex: -1 }), name ? button("Refresh", () => build(name, true)) : null);

// ---- 1. Project / source
view("project", async (root) => {
  const [st, src] = await Promise.all([api("/status"), api("/sources"), loadIndex()]);
  S.sources = src.sources;
  fillSourcePicker();
  const p = st.project || {};
  const res = st.resource_enforcement || {};
  const run = h("section.card", null, h("h3", { text: "Reference runs" }),
    note("Baseline evaluates the intact source on a split; analysis captures complete traces and builds GNATs from a baseline. Both run as jobs."));
  const label = textInput("intact-development"), split = selectEl([["development", "development"], ["locked-validation", "locked-validation"]], "development"), runLabel = textInput("intact-development");
  const out = h("div");
  run.append(h("div.row", null, field("Baseline label", label), field("Split", split),
    button("Run baseline", () => submitJob({ kind: "baseline", source: S.source, split: split.value, label: label.value.trim() }, `baseline ${label.value}`, (j) => out.replaceChildren(jobResult(j))).catch((e) => out.replaceChildren(errBox(e))))),
  h("div.row", null, field("Baseline to analyse", runLabel),
    button("Run analysis", () => submitJob({ kind: "analyse", run: runLabel.value.trim() }, `analyse ${runLabel.value}`, (j) => out.replaceChildren(jobResult(j))).catch((e) => out.replaceChildren(errBox(e))))), out);
  root.replaceChildren(heading("Project and sources", "project"),
    h("section.card", null, h("h3", { text: "Project" }), dl([["Path", p.path], ["Project ID", p.id], ["Schema", p.schema], ["Sources", p.sources], ["Results", p.results], ["Candidates", p.candidates],
      ["Selected candidate", p.selected ? code(p.selected) : "none yet"], ["Journal entries", p.journal_entries], ["Tool", `${st.tool} ${st.version}`], ["API", `${st.api?.version}, max page ${st.api?.max_page}`]]),
    h("h4", { text: "Resource enforcement" }), dl([["Effective", badge(res.effective)], ["systemd user scope", badge(res.systemd_user_scope)], ["cgroup v2", badge(res.cgroup_v2)], ["Worker threads", res.worker_threads]])),
    ...S.sources.map(sourceCard), run);
});
function sourceCard(s) {
  const sm = s.summary || {};
  const prov = h("div", null, note("Loading provenance record…", "loading"));
  object(s.record).then((r) => prov.replaceChildren(dl([["Input file", r.input_file], ["Input SHA-256", code(r.input_sha256)], ["Raw object", code(r.raw_object)], ["Topology object", code(r.topology_object)],
    ["Executable snapshot", r.snapshot_object ? code(r.snapshot_object) : "none (structural only)"], ["Tool version", r.tool_version]]),
  table("Model flags declared by the source", [["Flag", (x) => x[0]], ["Value", (x) => x[1]]], Object.entries(r.model_flags || {}))))
    .catch((e) => prov.replaceChildren(errBox(e)));
  return h("section.card", null, h("h3", null, "Source ", h("code", { text: s.name })),
    dl([["Format", s.format], ["Capability", badge(s.capability)], ["Record", code(s.record)], ["Input SHA-256", code(s.input_sha256)]]),
    capabilityBox(s.capability),
    table("Structure summary", [["Quantity", (x) => x[0]], ["Count", (x) => x[1]]], [["Sensory", sm.sensory], ["Hidden", sm.hidden], ["Outputs", sm.outputs], ["Graded cells", sm.graded_cells],
      ["Chemical synapses", sm.chemical_synapses], ["Excitatory", sm.excitatory], ["Inhibitory", sm.inhibitory], ["Electrical", sm.electrical]]),
    h("h4", { text: "Assumptions" }), ul(s.assumptions), h("h4", { text: "Limitations" }), ul(s.limitations), h("h4", { text: "Provenance" }), prov);
}
function jobResult(j) {
  if (j.state !== "succeeded") return errBox(`${j.label}: ${j.state}${j.error ? `: ${j.error}` : ""}`);
  return h("details.result", { open: true }, h("summary", { text: `${j.label}: result (job ${j.id})` }), h("pre", { text: JSON.stringify(j.result, null, 2) }));
}
function fillSourcePicker() {
  const sel = $("#global-source");
  if (!S.source || !S.sources.some((s) => s.name === S.source)) S.source = S.index?.defaults?.source || S.sources[0]?.name || null;
  sel.replaceChildren(...S.sources.map((s) => h("option", { value: s.name, text: `${s.name} (${s.capability})`, selected: s.name === S.source })));
  if (!S.sources.length) sel.append(h("option", { value: "", text: "No sources imported" }));
}

// ---- 2. Connectome
const CX = { offset: 0, limit: 500, page: null, pos: new Map(), cols: [], v: { s: 1, tx: 0, ty: 0, fitted: false }, cv: null, el: {} };
const W = 1000, HH = 620;
view("connectome", async (root) => {
  if (!S.source) { root.replaceChildren(heading("Connectome"), note("No source imported.")); return; }
  const q = h("input", { type: "search", placeholder: "ID, cell type or region", spellcheck: "false", autocomplete: "off", oninput: () => fillMatches() });
  const matches = h("select", { size: 8, onchange: () => selectNeuron(matches.value) });
  const size = selectEl([["250", "250"], ["500", "500"], ["1000", "1000"], ["2000", "2000"]], String(CX.limit), (v) => { CX.limit = +v; CX.offset = 0; loadCx(); });
  CX.cv = h("canvas.graph", { tabindex: 0, role: "img", "aria-label": "Connectome graph of the current page. Drag or use arrow keys to pan, wheel or plus and minus to zoom, 0 to fit. Choose neurons with the searchable list or the table." });
  CX.v.fitted = false;
  panZoom(CX.cv, CX.v, drawConnectome, (x, y, e) => {
    const id = hitNeuron(x, y);
    if (!id) return;
    if (e.shiftKey || e.ctrlKey || e.metaKey) changeBasket((b) => { b.neurons = b.neurons.includes(id) ? b.neurons.filter((n) => n !== id) : [...b.neurons, id]; }, `Toggled ${id} in the selection`);
    selectNeuron(id);
  });
  CX.el = { q, matches, hits: h("p.note", { "aria-live": "polite" }), note: h("p.note"), pager: h("span.pager", { "aria-live": "polite" }), detail: h("section.card.detail", { "aria-live": "polite" }), table: h("div"), layout: h("p.note") };
  root.replaceChildren(heading("Connectome", "connectome"),
    h("div.toolbar", null, field("Page size", size), button("Previous page", () => { CX.offset = Math.max(0, CX.offset - CX.limit); loadCx(); }), CX.el.pager, button("Next page", () => { CX.offset += CX.limit; loadCx(); })),
    CX.el.note, hlBanner(),
    h("div.split", null,
      h("div.canvas-box", null, CX.cv,
        h("div.row", null, button("Zoom in", () => { zoomAt(CX.v, CX.cv.clientWidth / 2, CX.cv.clientHeight / 2, 1.25); drawConnectome(); }), button("Zoom out", () => { zoomAt(CX.v, CX.cv.clientWidth / 2, CX.cv.clientHeight / 2, 0.8); drawConnectome(); }), button("Fit", () => { CX.v.fitted = false; drawConnectome(); })),
        legend([["exc", "Excitatory chemical (arrow head)"], ["inh", "Inhibitory chemical (bar head)"], ["gap", "Electrical (dashed, no head)"], ["sens", "Sensory"], ["hid", "Hidden"], ["out", "Output"], ["hi", "Highlighted GNAT/motif"], ["sel", "In intervention selection (ring)"]]),
        CX.el.layout, note("Shift-click a neuron to toggle it in the selection.")),
      h("div.side", null,
        field("Search all neurons", q, "Rust searches every neuron's ID, cell type and region (at most 200 hits)."), field("Matching neurons", matches), CX.el.hits,
        h("div.row", null, button("Add chosen neuron", () => matches.value && changeBasket((b) => b.neurons.push(matches.value), `Added ${matches.value}`)),
          button("Add all matches", () => { const ids = [...matches.options].map((o) => o.value); if (ids.length) changeBasket((b) => b.neurons.push(...ids), `Added ${ids.length} neurons`); })),
        CX.el.detail, basketBox())),
    CX.el.table);
  await loadCx();
  fillMatches();
}, () => { drawConnectome(); });
async function loadCx() {
  const p = await topoPage(S.source, CX.offset, CX.limit).catch((e) => { CX.el.note.replaceChildren(errBox(e)); return null; });
  if (!p) return;
  if (CX.offset >= p.total_neurons && p.total_neurons > 0) { CX.offset = Math.max(0, Math.floor((p.total_neurons - 1) / CX.limit) * CX.limit); return loadCx(); }
  CX.page = p;
  CX.v.fitted = false;
  const n = p.neurons.length, total = p.total_neurons, pages = Math.max(1, Math.ceil(total / CX.limit));
  CX.el.pager.textContent = `Page ${Math.floor(CX.offset / CX.limit) + 1} of ${pages}`;
  const capped = p.synapses.length >= p.limit * 4;
  CX.el.note.textContent = `${p.display_note}. Showing neurons ${n ? CX.offset + 1 : 0}–${CX.offset + n} of ${total}; ${p.synapses.length} of ${p.total_synapses} synapses are drawn (only those with both endpoints on this page${capped ? "; the synapse list hit the per-page cap" : ""}). Capability: ${p.capability}.`;
  layoutCx(p);
  CX.deg = { pre: new Map(), post: new Map() };
  for (const s of topoCache(S.source).syn.values()) for (const e of ["pre", "post"]) CX.deg[e].set(s[e], (CX.deg[e].get(s[e]) || 0) + 1);
  CX.el.table.replaceChildren(table(`Neurons on this page (tabular alternative to the graph; ${n} rows)`, [
    ["ID", (x) => h("code", { text: x.id })], ["Role", (x) => x.role], ["Type", (x) => x.type || "–"], ["Region", (x) => x.region], ["Graded", (x) => x.graded],
    ["Position", (x) => x.position_kind], ["In (this page)", (x) => degree(x.id, "post")], ["Out (this page)", (x) => degree(x.id, "pre")],
    ["Inspect", (x) => button("Inspect", () => selectNeuron(x.id), { cls: "small" })], ["Selection", (x) => togButton(x.id, "neurons")]], p.neurons, { max: 2000 }));
  renderDetail();
  drawConnectome();
}
const degree = (id, end) => CX.deg[end].get(id) || 0;
function layoutCx(p) {
  const ns = p.neurons, pos = new Map(), cols = [];
  const kinds = [...new Set(ns.map((n) => n.position_kind))].join(", ") || "none";
  const real = ns.length && ns.every((n) => Array.isArray(n.position) && (n.position_kind === "measured" || n.position_kind === "inferred"));
  if (real) {
    const xs = ns.map((n) => n.position[0]), ys = ns.map((n) => n.position[1]);
    const [x0, x1, y0, y1] = [Math.min(...xs), Math.max(...xs), Math.min(...ys), Math.max(...ys)], sc = Math.min(W / (x1 - x0 || 1), (HH - 60) / (y1 - y0 || 1));
    for (const n of ns) pos.set(n.id, [(n.position[0] - x0) * sc, 40 + (n.position[1] - y0) * sc]);
    CX.el.layout.textContent = `Layout: ${kinds} positions, x–y projection of the 3-D coordinates (inferred geometry is not measured anatomy).`;
  } else {
    const groups = new Map([["Sensory", []]]);
    for (const n of ns) if (n.role === "hidden") { const r = n.region || "(no region)"; if (!groups.has(r)) groups.set(r, []); groups.get(r).push(n); }
    groups.get("Sensory").push(...ns.filter((n) => n.role === "sensory"));
    groups.set("Outputs", ns.filter((n) => n.role === "output"));
    const list = [...groups].filter((g) => g[1].length), maxRows = 28, units = list.map((g) => Math.ceil(g[1].length / maxRows));
    const total = units.reduce((a, b) => a + b, 0) + 0.7 * (list.length - 1);
    let cur = 0;
    list.forEach(([name, arr], gi) => {
      const rows = Math.min(maxRows, arr.length);
      arr.forEach((n, i) => pos.set(n.id, [((cur + Math.floor(i / maxRows) + 0.5) / total) * W, 50 + (((i % maxRows) + 1) / (rows + 1)) * (HH - 60)]));
      cols.push({ text: name, x: ((cur + units[gi] / 2) / total) * W });
      cur += units[gi] + 0.7;
    });
    CX.el.layout.textContent = `Layout: procedural grouping by role (sensory left, hidden by region, outputs right). This is not anatomy; declared position kinds: ${kinds}.`;
  }
  CX.pos = pos;
  CX.cols = cols;
}
function drawConnectome() {
  const cv = CX.cv, p = CX.page;
  if (!cv || !p || !cv.isConnected || cv.clientWidth === 0) return;
  const { ctx, w, h: hh } = prep(cv, 540), C = colours(), v = CX.v;
  if (!v.fitted) { v.s = Math.min(w / W, hh / HH) * 0.94; v.tx = (w - W * v.s) / 2; v.ty = (hh - HH * v.s) / 2; v.fit = v.s; v.fitted = true; }
  const P = (id) => { const q = CX.pos.get(id); return q && [q[0] * v.s + v.tx, q[1] * v.s + v.ty]; };
  const r = Math.max(3, Math.min(13, 6.5 * (v.s / v.fit))), sel = S.selected;
  const hi = new Set(S.highlight ? S.highlight.neurons : []), bn = new Set(S.basket.neurons), bs = new Set(S.basket.synapses);
  ctx.fillStyle = C.muted; ctx.font = "12px system-ui, sans-serif"; ctx.textAlign = "center";
  for (const c of CX.cols) ctx.fillText(c.text, c.x * v.s + v.tx, 22 * v.s + v.ty);
  ctx.textAlign = "left";
  for (const s of p.synapses) {
    const a = P(s.pre), b = P(s.post);
    if (!a || !b) continue;
    const elec = s.class === "electrical", inh = !elec && s.weight < 0;
    ctx.globalAlpha = !sel || s.pre === sel || s.post === sel ? 0.9 : 0.14;
    ctx.strokeStyle = ctx.fillStyle = elec ? C.gap : inh ? C.inh : C.exc;
    ctx.lineWidth = (0.7 + Math.min(3, Math.log1p(Math.abs(s.weight)) / 1.4)) * (bs.has(s.id) ? 2.2 : 1);
    ctx.setLineDash(elec ? [5, 4] : []);
    ctx.beginPath();
    if (s.pre === s.post) { ctx.arc(a[0], a[1] - r * 1.6, r * 1.1, 0, Math.PI * 2); ctx.stroke(); continue; }
    const dx = b[0] - a[0], dy = b[1] - a[1], cx = (a[0] + b[0]) / 2 - dy * 0.12, cy = (a[1] + b[1]) / 2 + dx * 0.12;
    ctx.moveTo(a[0], a[1]);
    ctx.quadraticCurveTo(cx, cy, b[0], b[1]);
    ctx.stroke();
    if (elec) continue;
    const tx = b[0] - cx, ty = b[1] - cy, tl = Math.hypot(tx, ty) || 1, ux = tx / tl, uy = ty / tl, ex = b[0] - ux * (r + 2), ey = b[1] - uy * (r + 2), k = Math.max(4, r * 0.8);
    ctx.setLineDash([]);
    ctx.beginPath();
    if (inh) { ctx.moveTo(ex - uy * k, ey + ux * k); ctx.lineTo(ex + uy * k, ey - ux * k); ctx.stroke(); } else { ctx.moveTo(ex, ey); ctx.lineTo(ex - ux * k * 1.6 - uy * k * 0.8, ey - uy * k * 1.6 + ux * k * 0.8); ctx.lineTo(ex - ux * k * 1.6 + uy * k * 0.8, ey - uy * k * 1.6 - ux * k * 0.8); ctx.closePath(); ctx.fill(); }
  }
  ctx.globalAlpha = 1;
  ctx.setLineDash([]);
  const labels = p.neurons.length <= 140 || v.s / v.fit > 1.8;
  ctx.font = "11px system-ui, sans-serif";
  for (const n of p.neurons) {
    const q = P(n.id);
    if (!q) continue;
    ctx.beginPath();
    ctx.arc(q[0], q[1], r, 0, Math.PI * 2);
    ctx.fillStyle = hi.has(n.id) ? C.hi : n.role === "sensory" ? C.sens : n.role === "output" ? C.out : C.hid;
    ctx.fill();
    ctx.lineWidth = 1; ctx.strokeStyle = C.fg; ctx.stroke();
    if (bn.has(n.id)) { ctx.beginPath(); ctx.arc(q[0], q[1], r + 3.5, 0, Math.PI * 2); ctx.lineWidth = 2.5; ctx.strokeStyle = C.accent; ctx.stroke(); }
    if (n.id === sel) { ctx.beginPath(); ctx.arc(q[0], q[1], r + 7, 0, Math.PI * 2); ctx.lineWidth = 2; ctx.strokeStyle = C.fg; ctx.setLineDash([3, 2]); ctx.stroke(); ctx.setLineDash([]); }
    if (labels) { ctx.fillStyle = C.fg; ctx.fillText(n.id, q[0] + r + 3, q[1] - r + 1); }
  }
  const off = [...hi].filter((id) => !CX.pos.has(id)).length;
  if (off) { ctx.fillStyle = C.muted; ctx.fillText(`${off} highlighted neurons are not on this page`, 8, hh - 8); }
}
function hitNeuron(x, y) {
  const v = CX.v;
  let best = null, bd = Math.max(10, 6.5 * (v.s / v.fit) + 4);
  for (const [id, q] of CX.pos) { const d = Math.hypot(q[0] * v.s + v.tx - x, q[1] * v.s + v.ty - y); if (d < bd) { bd = d; best = id; } }
  return best;
}
let searchTimer = null, searchSeq = 0;
function fillMatches() {
  clearTimeout(searchTimer);
  searchTimer = setTimeout(async () => {
    const el = CX.el.matches, seq = ++searchSeq;
    if (!el) return;
    try {
      const r = await api(`/search?source=${enc(S.source)}&q=${enc(CX.el.q.value.trim())}`);
      if (seq !== searchSeq) return;
      el.replaceChildren(...r.hits.map((n) => h("option", { value: n.id, text: `${n.id} · ${n.role} · ${n.type || "untyped"} · ${n.region}`, selected: n.id === S.selected })));
      const total = topoCache(S.source).meta?.total_neurons ?? "?";
      CX.el.hits.textContent = r.hits.length >= r.limit ? `The first ${r.limit} matches among all ${total} neurons (search limit reached; refine the query).` : `${r.hits.length} matches among all ${total} neurons.`;
    } catch (e) { CX.el.hits.textContent = e.message; }
  }, 200);
}
function selectNeuron(id) {
  S.selected = id;
  renderDetail();
  drawConnectome();
}
const neuronCache = new Map();
function neuronInfo(src, id) {
  const k = `${src}\u0000${id}`;
  if (!neuronCache.has(k)) neuronCache.set(k, api(`/neuron?source=${enc(src)}&id=${enc(id)}`).catch((e) => { neuronCache.delete(k); throw e; }));
  return neuronCache.get(k);
}
async function renderDetail() {
  const box = CX.el.detail, id = S.selected;
  if (!box) return;
  if (!id) { box.replaceChildren(note("Choose a neuron to inspect its synapses.")); return; }
  box.replaceChildren(note(`Loading neuron ${id}…`, "loading"));
  let r;
  try { r = await neuronInfo(S.source, id); } catch (e) { box.replaceChildren(errBox(e)); return; }
  if (S.selected !== id || !box.isConnected) return;
  const n = { ...(topoCache(S.source).byId.get(id) || {}), ...r.neuron }, cap = 8000;
  const cols = (end) => [["Synapse", (s) => h("code", { text: s.id })], [end === "pre" ? "From" : "To", (s) => h("code", { text: s[end] })], ["Class", (s) => s.class],
    ["Sign", (s) => s.class === "electrical" ? "electrical" : s.weight < 0 ? "inhibitory" : "excitatory"], ["Weight", (s) => s.weight], ["Delay (ticks)", (s) => s.delay_ticks], ["Selection", (s) => togButton(s.id, "synapses")]];
  box.replaceChildren(h("h3", null, "Neuron ", h("code", { text: n.id })),
    dl([["Global index", n.index], ["Role", n.role], ["Cell type", n.type || "–"], ["Region", n.region], ["Graded", n.graded], n.position_kind && ["Position", n.position ? n.position.map(fmt).join(", ") : "none"], n.position_kind && ["Position kind", n.position_kind]]),
    h("div.row", null, togButton(n.id, "neurons"), CX.pos.has(n.id) ? null : button("Show its page in the graph", () => { CX.offset = Math.floor(n.index / CX.limit) * CX.limit; loadCx(); })),
    table(`Incoming synapses of ${n.id} (${r.incoming.length})`, cols("pre"), r.incoming),
    table(`Outgoing synapses of ${n.id} (${r.outgoing.length})`, cols("post"), r.outgoing),
    note(`All synapses of this neuron across the whole source${r.incoming.length >= cap || r.outgoing.length >= cap ? `; a list reached the server cap of ${cap}, so it may be truncated` : ""}.`));
}

// ---- 3. Activity
const AX = { an: null, anName: null, key: null, tr: null, man: null, all: null, events: [], a: 0, w: 120, play: null, cv: null, el: {}, rows: [], src: null, seq: 0 };
view("activity", async (root) => {
  await loadIndex();
  const ans = entries("analyses");
  if (!ans.length) { root.replaceChildren(heading("Activity", "activity"), note("No analysis yet. Run a baseline and an analysis in the Project view: trace manifests are listed in the analysis record.")); return; }
  const hlA = S.highlight?.analysis && S.index.analyses[S.highlight.analysis] ? S.highlight.analysis : null;
  const anSel = selectEl(ans.map(([n]) => [n, n]), hlA || (AX.anName && S.index.analyses[AX.anName] ? AX.anName : ans[0][0]), (v) => loadAnalysis(v));
  const trSel = h("select", { onchange: () => loadTrace(trSel.value) });
  const start = h("input", { type: "range", min: 0, max: 0, step: 1, value: "0", oninput: () => { AX.a = +start.value; windowChanged(); } });
  const width = numInput(AX.w, { min: 1, step: 1 });
  width.addEventListener("change", () => { AX.w = Math.max(1, Math.round(+width.value) || 1); windowChanged(); });
  const play = button("Play", () => togglePlay(), { pressed: "false" });
  const gn = h("select", { onchange: () => { const g = AX.gnats?.find((x) => x.id === gn.value); setHighlight(g ? gnatHl(AX.task, g, "GNAT", AX.anName) : null); } });
  AX.cv = h("canvas.raster", { tabindex: 0, role: "img", "aria-label": "Spike raster: x is tick, y is neuron. Click a row to select that neuron. The event tables below are the tabular alternative." });
  AX.cv.addEventListener("click", (e) => { const r = AX.cv.getBoundingClientRect(), row = Math.floor((e.clientY - r.top - AX.geom.top) / AX.geom.rh); const idx = AX.rows[row]; if (idx != null) { S.selected = neuronName(AX.src, idx); AX.el.pick.textContent = `Selected neuron ${S.selected} (index ${idx}).`; drawRaster(); } });
  AX.el = { anSel, trSel, start, width, play, gn, info: h("div"), pick: h("p.note", { "aria-live": "polite" }), root: h("p.note"), legend: h("div"), win: h("p.note", { "aria-live": "polite" }), tables: h("div"), gnNote: h("p.note") };
  root.replaceChildren(heading("Activity", "activity"),
    h("div.toolbar", null, field("Analysis", anSel), field("Trace (task/seed)", trSel), field("Overlay GNAT (first trial)", gn)),
    h("div.toolbar", null, field("Window start (tick)", start), field("Window width (ticks)", width), play, button("Full trace", () => { AX.a = 0; AX.w = AX.tr ? AX.tr.ticks : AX.w; width.value = String(AX.w); start.value = "0"; windowChanged(); })),
    AX.el.win, AX.el.gnNote, AX.el.root, hlBanner(), AX.cv, AX.el.legend, AX.el.pick, AX.el.info, AX.el.tables);
  await loadAnalysis(anSel.value);
}, () => { if (!followHighlight()) drawRaster(); });
// A highlight from the GNAT view carries its trial's manifest when the analysis records it.
const gnatHl = (t, g, kind, analysis) => ({ kind, analysis, id: g.id || g.fingerprint, task: t.task, neurons: g.neurons, start_tick: g.start_tick ?? 0, end_tick: g.end_tick ?? -1, open_start: g.open_start, open_end: g.open_end,
  event_ids: kind === "GNAT" ? g.event_ids : undefined, event_ids_truncated: g.event_ids_truncated, trace: kind === "GNAT" ? t.first_trial?.trace : null, seed: t.first_trial?.seed });
const keyOfTrace = (hash) => Object.keys(AX.an?.traces || {}).find((k) => AX.an.traces[k] === hash);
const traceKeyFor = (x) => (x.trace && keyOfTrace(x.trace)) || Object.keys(AX.an.traces || {}).filter((t) => t.startsWith(`${x.task}/`)).sort(natural)[0];
function followHighlight() {
  const x = S.highlight;
  if (!x || !AX.tr || !AX.an) return false;
  if (x.analysis && x.analysis !== AX.anName && S.index.analyses[x.analysis]) {
    AX.el.anSel.value = x.analysis;
    announce(`Activity switched to analysis ${x.analysis} to show the highlighted ${x.kind}`);
    loadAnalysis(x.analysis);
    return true;
  }
  if (!x.trace && x.task === AX.tr.task) return false;
  const k = traceKeyFor(x);
  if (!k || k === AX.key) return false;
  AX.el.trSel.value = k;
  announce(`Activity switched to trace ${k} to show the highlighted ${x.kind} from task ${x.task}`);
  loadTrace(k);
  return true;
}
async function loadAnalysis(name) {
  AX.anName = name;
  AX.an = await object(S.index.analyses[name]);
  AX.src = AX.an.source;
  const keys = Object.keys(AX.an.traces || {}).sort(natural);
  AX.el.trSel.replaceChildren(...keys.map((k) => h("option", { value: k, text: k })));
  if (!keys.length) { AX.el.info.replaceChildren(note("This analysis lists no trace manifests.")); return; }
  const want = S.highlight && traceKeyFor(S.highlight);
  AX.el.trSel.value = want || keys[0];
  await loadTrace(AX.el.trSel.value);
}
async function loadTrace(key) {
  const hash = AX.an.traces[key], seq = ++AX.seq;
  AX.key = key;
  AX.el.info.replaceChildren(note("Loading trace…", "loading"));
  const [tr, man] = await Promise.all([api(`/trace?manifest=${enc(hash)}`), object(hash).catch(() => null)]);
  if (seq !== AX.seq) return;
  const t = topoCache(AX.src);
  if (!t.meta) await topoPage(AX.src, 0, 2000);
  const total = t.meta.total_neurons;
  const ni = tr.fields.indexOf("neuron");
  await ensureIndices(AX.src, total <= 300 ? [...Array(total).keys()] : new Set(tr.events.map((e) => e[ni])));
  Object.assign(AX, { tr, man, all: tr.truncated_for_display ? null : tr.events, F: Object.fromEntries(tr.fields.map((f, i) => [f, i])) });
  if (AX.w > tr.ticks || AX.a >= tr.ticks) { AX.a = 0; AX.w = tr.ticks; }
  AX.el.start.max = String(Math.max(0, tr.ticks - 1));
  AX.el.start.value = String(AX.a);
  AX.el.width.value = String(AX.w);
  const task = AX.an.tasks.find((x) => x.task === tr.task), seeds = Object.keys(AX.an.traces).filter((k) => k.startsWith(`${tr.task}/`)).map((k) => k.slice(tr.task.length + 1)).sort(natural);
  AX.task = task || { task: tr.task };
  AX.gnats = task?.gnats_first_trial || [];
  AX.el.gn.replaceChildren(h("option", { value: "", text: "None" }), ...AX.gnats.map((g) => h("option", { value: g.id, text: `${g.id} (${g.neurons.length} neurons, ticks ${g.start_tick}–${g.end_tick})`, selected: S.highlight?.id === g.id })));
  const ft = task?.first_trial, exact = AX.gnats.some((g) => Array.isArray(g.event_ids));
  AX.el.gnNote.textContent = (ft?.trace ? `GNATs are listed for the first trial: seed ${ft.seed}, trace ${keyOfTrace(ft.trace) || ft.trace} (recorded by the analysis).`
    : `GNATs are listed for the first trial only; this older analysis does not record which, so this page assumes the lowest seed (${seeds[0] ?? "?"}) and checks each GNAT's root event against the loaded trace.`)
    + (exact ? " Highlighted member events are exact (event IDs from the analysis)." : " This analysis lists no GNAT event IDs, so the highlight marks events of GNAT neurons inside the GNAT's tick window (approximate); event colours are by kind.");
  const cov = Object.entries(tr.coverage || {});
  AX.el.info.replaceChildren(h("section.card", null, h("h3", { text: `Trace ${key}` }),
    dl([["Manifest", code(hash)], ["Task / seed", `${tr.task} / ${tr.seed}`], ["Ticks recorded", `0 to ${tr.ticks} (recording boundaries)`], ["Complete", badge(tr.complete ? "complete" : "incomplete")],
      ["Display truncation", tr.truncated_for_display ? badge("truncated: the full-range response hit the event limit; windows are refetched") : badge("complete: no display truncation")],
      man && ["Mode", man.mode], man && ["Time semantics", man.time_semantics], man && ["Plasticity policy", man.plasticity_policy], man && ["Warm-up ticks", man.warmup_ticks],
      man && ["Gaps", man.gaps?.length ? man.gaps.map((g) => g.join("–")).join(", ") : "none"], man && ["Excluded events", `${man.excluded_events}${man.exclusion_reasons?.length ? `: ${man.exclusion_reasons.join("; ")}` : ""}`]]),
    table("Coverage by stream", [["Stream", (x) => x[0]], ["Coverage", (x) => badge(x[1])]], cov)));
  windowChanged();
}
let winTimer = null;
function windowChanged() {
  if (!AX.tr) return;
  AX.el.start.value = String(AX.a);
  const b = AX.a + AX.w;
  if (AX.all) { AX.events = AX.all.filter((e) => e[AX.F.tick] >= AX.a && e[AX.F.tick] < b); AX.winTrunc = false; renderWindow(); return; }
  clearTimeout(winTimer);
  winTimer = setTimeout(async () => {
    const seq = AX.seq;
    try {
      const r = await api(`/trace?manifest=${enc(AX.an.traces[AX.key])}&from=${AX.a}&to=${b}`);
      if (seq !== AX.seq) return;
      await ensureIndices(AX.src, new Set(r.events.map((e) => e[AX.F.neuron])));
      AX.events = r.events;
      AX.winTrunc = r.truncated_for_display;
      renderWindow();
    } catch (e) { AX.el.win.replaceChildren(errBox(e)); }
  }, 200);
}
function renderWindow() {
  const F = AX.F, t = topoCache(AX.src), total = t.meta.total_neurons;
  AX.rows = total <= 300 ? [...Array(total).keys()] : [...new Set(AX.events.map((e) => e[F.neuron]))].sort((a, b) => a - b);
  AX.el.win.textContent = `Window: ticks ${AX.a}–${AX.a + AX.w - 1} of 0–${AX.tr.ticks - 1}; ${AX.events.length} events${AX.winTrunc ? " (truncated_for_display: this window hit the event limit)" : ""}. ${total <= 300 ? "Rows: every neuron in index order." : "Rows: only neurons with events in this window."}`;
  drawRaster();
  const counts = new Map();
  for (const e of AX.events) { const c = counts.get(e[F.neuron]) || [0, 0]; c[e[F.kind] === 0 ? 0 : 1]++; counts.set(e[F.neuron], c); }
  AX.el.tables.replaceChildren(
    table("Events per neuron in this window", [["Neuron", (x) => h("code", { text: neuronName(AX.src, x[0]) })], ["Index", (x) => x[0]], ["Role", (x) => t.byIndex.get(x[0])?.role], ["Spikes", (x) => x[1][0]], ["Graded/other", (x) => x[1][1]]], [...counts].sort((a, b) => a[0] - b[0])),
    table("Events in this window", [["Event ID", (e) => String(e[F.id])], ["Tick", (e) => e[F.tick]], ["Micro", (e) => e[F.micro]], ["Neuron", (e) => h("code", { text: neuronName(AX.src, e[F.neuron]) })], ["Kind", (e) => e[F.kind] === 0 ? "spike" : "graded/other"]], AX.events, { max: 300 }));
}
function togglePlay() {
  const b = AX.el.play;
  if (AX.play) { clearInterval(AX.play); AX.play = null; b.textContent = "Play"; b.setAttribute("aria-pressed", "false"); return; }
  b.textContent = "Pause playback";
  b.setAttribute("aria-pressed", "true");
  if (AX.a + AX.w >= AX.tr.ticks) AX.a = 0;
  if (AX.w >= AX.tr.ticks) { AX.w = Math.max(10, Math.round(AX.tr.ticks / 5)); AX.el.width.value = String(AX.w); }
  AX.play = setInterval(() => {
    if (!AX.tr || AX.a + AX.w >= AX.tr.ticks || !AX.cv.isConnected) { togglePlay(); return; }
    AX.a = Math.min(AX.tr.ticks - AX.w, AX.a + Math.max(1, Math.round(AX.w / 10)));
    windowChanged();
  }, 300);
}
function drawRaster() {
  const cv = AX.cv;
  if (!cv || !AX.tr || !cv.isConnected || cv.clientWidth === 0) return;
  const F = AX.F, n = AX.rows.length, rh = Math.max(3, Math.min(16, 640 / Math.max(1, n))), top = 10, H = top + n * rh + 40;
  const { ctx, w } = prep(cv, Math.max(120, H)), C = colours(), left = 78, right = w - 10, a = AX.a, b = AX.a + AX.w;
  AX.geom = { top, rh };
  const X = (t) => left + ((t - a) / (b - a)) * (right - left), rowOf = new Map(AX.rows.map((x, i) => [x, i])), tt = topoCache(AX.src);
  ctx.font = "11px system-ui, sans-serif";
  const step = Math.ceil(11 / rh);
  AX.rows.forEach((idx, i) => {
    const nn = tt.byIndex.get(idx);
    if (i % 2) { ctx.fillStyle = C.grid; ctx.globalAlpha = 0.35; ctx.fillRect(left, top + i * rh, right - left, rh); ctx.globalAlpha = 1; }
    if (i % step === 0) { ctx.fillStyle = nn?.id === S.selected ? C.accent : C.fg; ctx.fillText((nn?.id ?? `#${idx}`).slice(0, 11), 4, top + i * rh + rh / 2 + 4); }
  });
  axes(ctx, C, { l: left, r: right, t: top, b: top + n * rh }, [a, b], null, "tick");
  ctx.setLineDash([4, 3]); ctx.strokeStyle = C.muted;
  for (const t of [0, AX.tr.ticks]) if (t >= a && t <= b) { ctx.beginPath(); ctx.moveTo(X(t), top); ctx.lineTo(X(t), top + n * rh); ctx.stroke(); }
  ctx.setLineDash([]);
  const g = S.highlight && S.highlight.task === AX.tr.task ? S.highlight : null, gset = new Set();
  const other = g && g.trace && g.trace !== AX.an.traces[AX.key], show = g && !other;
  const exact = show && Array.isArray(g.event_ids) ? new Set(g.event_ids) : null;
  if (show) {
    for (const id of g.neurons) { const nn = tt.byId.get(id); if (nn && rowOf.has(nn.index)) gset.add(nn.index); }
    const x0 = X(Math.max(a, g.start_tick)), x1 = X(Math.min(b, g.end_tick + 1));
    ctx.fillStyle = C.hi; ctx.globalAlpha = exact ? 0.16 : 0.28;
    if (x1 > x0) for (const i of gset) ctx.fillRect(x0, top + rowOf.get(i) * rh, x1 - x0, rh);
    ctx.globalAlpha = 1; ctx.fillStyle = C.fg;
    if (g.open_start) ctx.fillText("◀ open start", x0 + 2, top + n * rh - 2);
    if (g.open_end) ctx.fillText("open end ▶", x1 - 62, top + n * rh - 2);
  }
  const pw = Math.max(1.5, Math.min(6, ((right - left) / (b - a)) * 0.7));
  let members = 0;
  for (const e of AX.events) {
    const r = rowOf.get(e[F.neuron]);
    if (r == null) continue;
    const inG = exact ? exact.has(String(e[F.id])) : show && gset.has(e[F.neuron]) && e[F.tick] >= g.start_tick && e[F.tick] <= g.end_tick, x = X(e[F.tick] + 0.5) - pw / 2, y = top + r * rh;
    if (inG) members++;
    ctx.fillStyle = inG ? C.fg : e[F.kind] === 0 ? C.exc : C.gap;
    if (e[F.kind] === 0) ctx.fillRect(x, y + 1, pw, rh - 2); else ctx.fillRect(x, y + rh / 2 - pw / 2, pw, pw);
  }
  const lg = exact ? "Highlighted GNAT: member events drawn dark on its window band (exact, from event IDs)" : "Highlighted GNAT: window band; dark marks are events of its neurons inside the window (approximate)";
  if (AX.lg !== lg) { AX.lg = lg; AX.el.legend.replaceChildren(legend([["exc", "Spike (vertical tick)"], ["gap", "Graded or other event (square)"], ["hi", lg], ["bound", "Recording boundary (dashed)"]])); }
  if (!g) AX.el.root.textContent = "";
  else if (other) AX.el.root.textContent = `${g.id} belongs to the first trial (seed ${g.seed}, trace ${keyOfTrace(g.trace) || g.trace}); this is another trial, so nothing is overlaid.`;
  else if (exact) AX.el.root.textContent = `Exact membership of ${g.id}: ${g.event_ids.length} member event IDs from the analysis${g.event_ids_truncated ? " (Rust truncated the list to the first 2000; later members are not marked)" : ""}; ${members} of them in this window.`;
  else {
    const m = /^gnat-t(\d+)-n(\d+)-e(\d+)$/.exec(g.id), all = AX.all || AX.events;
    const ok = m && all.some((e) => String(e[F.tick]) === m[1] && String(e[F.neuron]) === m[2] && String(e[F.id]) === m[3]);
    AX.el.root.textContent = m ? (ok ? `Root event of ${g.id} found in this trace: the overlay belongs to this trial (approximate membership).` : `Root event of ${g.id} is not in this trace${AX.all ? "" : " window"}: the GNAT may belong to another trial.`) : "";
  }
}

// ---- 4. GNATs and motifs
const GX = { name: null, an: null, task: null, bars: null, part: [] };
view("gnats", async (root) => {
  await loadIndex();
  const ans = entries("analyses");
  if (!ans.length) { root.replaceChildren(heading("GNATs and motifs", "gnats"), note("No analysis yet. Run one from the Project view.")); return; }
  GX.name = GX.name && S.index.analyses[GX.name] ? GX.name : ans[0][0];
  const sel = selectEl(ans.map(([n]) => [n, n]), GX.name, (v) => { GX.name = v; build("gnats"); });
  const an = GX.an = await object(S.index.analyses[GX.name]);
  const tasks = an.tasks || [];
  GX.task = tasks.some((t) => t.task === GX.task) ? GX.task : tasks[0]?.task;
  const taskBox = h("div");
  const tsel = selectEl(tasks.map((t) => [t.task, `${t.task} (${(t.gnats_first_trial || []).length} GNATs, ${(t.motifs || []).length} motifs)`]), GX.task, (v) => { GX.task = v; fillTask(taskBox); });
  GX.part = [...(an.participation || [])].sort((a, b) => b.events_in_gnats - a.events_in_gnats || natural(a.id, b.id));
  GX.bars = h("canvas.bars", { role: "img", "aria-label": "Bar chart of events inside GNATs per hidden neuron; the participation table follows." });
  const cal = an.calibration || {};
  root.replaceChildren(heading("GNATs and motifs", "gnats"), h("div.toolbar", null, field("Analysis", sel)),
    h("section.card", null, dl([["Run", an.run], ["Source", an.source], ["Method", an.method], ["Norm", an.norm], ["τ (ms)", an.params?.tau_ms], ["Threshold", an.params?.threshold], ["dt (ms)", an.params?.dt_ms], ["Traces", Object.keys(an.traces || {}).length], ["Coverage", an.coverage_note]]),
      note("GNATs and motifs are activity-derived structure under the declared method; they are not evidence of biological validity.")),
    hlBanner(),
    table("Tasks", [["Task", (t) => t.task], ["Trials", (t) => t.trials], ["First-trial seed", (t) => t.first_trial?.seed != null ? String(t.first_trial.seed) : "not recorded (older analysis)"], ["GNATs (first trial)", (t) => (t.gnats_first_trial || []).length], ["Motifs", (t) => (t.motifs || []).length], ["Recurrence", (t) => t.recurrence_approximate ? badge("approximate (candidate budget hit)") : badge("passed: bounded-verified")]], tasks),
    h("div.toolbar", null, field("Task", tsel)), taskBox,
    h("section.card", null, h("h3", { text: "Participation of hidden neurons" }), GX.bars,
      table("Participation (events inside GNATs, all tasks and trials)", [["Neuron", (p) => h("code", { text: p.id })], ["Region", (p) => p.region], ["Events in GNATs", (p) => p.events_in_gnats], ["GNATs", (p) => p.gnats], ["Selection", (p) => togButton(p.id, "neurons")]], GX.part)),
    h("section.card", null, h("h3", { text: "Calibration and sensitivity" }),
      dl([["Status", badge(cal.status)], ["False-edge fraction", cal.false_edge_fraction], ["Suggested threshold", cal.suggested_threshold], ["Shuffles", cal.shuffles], ["Seed", cal.seed != null ? String(cal.seed) : null], ["Histogram bin", cal.bin]]),
      table("Sensitivity of GNAT structure to τ and threshold", [["τ (ms)", (s) => s.tau_ms], ["Threshold", (s) => s.threshold], ["GNATs", (s) => s.gnats], ["Edges", (s) => s.edges], ["Max depth", (s) => s.max_depth], ["Mean size", (s) => s.mean_size], ["Isolated events", (s) => s.isolated_events]], an.sensitivity || [])));
  fillTask(taskBox);
  drawBars();
}, () => drawBars());
function fillTask(box) {
  const t = GX.an.tasks.find((x) => x.task === GX.task);
  if (!t) { box.replaceChildren(note("No task selected.")); return; }
  const hl = (kind, x) => button("Highlight", () => setHighlight(gnatHl(t, x, kind, GX.name)), { cls: "small" });
  const addAll = (x) => button("Select neurons", () => changeBasket((b) => b.neurons.push(...x.neurons), `Added ${x.neurons.length} neurons`), { cls: "small" });
  const bound = (g) => g.open_start || g.open_end ? badge(["open", g.open_start && "start (may have unseen ancestors)", g.open_end && "end (may have unseen descendants)"].filter(Boolean).join(" ")) : badge("passed: closed");
  box.replaceChildren(
    table(`Depth summary per trial: ${t.task}`, [["Trial", (d, i) => i + 1], ["GNATs", (d) => d.gnats], ["Closed", (d) => d.closed_gnats], ["Events in GNATs", (d) => d.events_in_gnats], ["Isolated events", (d) => d.isolated_events], ["Max depth", (d) => d.max_depth], ["Mean depth", (d) => d.mean_depth], ["p95 depth", (d) => d.p95_depth], ["Mean size", (d) => d.mean_size], ["Participating neurons", (d) => d.participating_neurons]], t.depth || []),
    table(`GNATs of the first trial: ${t.task} (the record keeps at most 50)`, [["GNAT", (g) => h("code", { text: g.id })], ["Neurons", (g) => g.neurons.join(", ")], ["Events", (g) => g.events], ["Depth", (g) => g.depth], ["Ticks", (g) => `${g.start_tick}–${g.end_tick}`], ["Boundary", bound],
      ["Member event IDs", (g) => Array.isArray(g.event_ids) ? `${g.event_ids.length}${g.event_ids_truncated ? ` of ${g.events} (truncated)` : " (exact)"}` : "not recorded (older analysis)"], ["Show", (g) => hl("GNAT", g)], ["Selection", addAll]], t.gnats_first_trial || [], { empty: "No GNATs in the first trial of this task." }),
    table(`Recurring motifs: ${t.task} (first trial against later trials; at most 20 kept)${t.recurrence_approximate ? "; approximate" : ""}`, [["Fingerprint", (m) => h("code", { text: m.fingerprint })], ["Occurrences", (m) => m.occurrences], ["Neurons", (m) => m.neurons.join(", ")], ["Edges", (m) => m.edges.join(", ")], ["Mean timing jitter (ticks)", (m) => m.mean_timing_jitter_ticks], ["Collisions separated", (m) => m.collisions_separated], ["Show", (m) => hl("motif", m)], ["Selection", addAll]], t.motifs || [], { empty: "No recurring motifs for this task." }));
}
function drawBars() {
  const cv = GX.bars;
  if (!cv || !cv.isConnected || cv.clientWidth === 0) return;
  const items = GX.part.slice(0, 60), rh = 16, { ctx, w } = prep(cv, 30 + items.length * rh + 20), C = colours(), left = 80, right = w - 16;
  const max = Math.max(1, ...items.map((p) => p.events_in_gnats)), hi = new Set(S.highlight ? S.highlight.neurons : []);
  const { X } = axes(ctx, C, { l: left, r: right, t: 8, b: 8 + items.length * rh }, [0, max], null, "events inside GNATs");
  items.forEach((p, i) => {
    ctx.fillStyle = hi.has(p.id) ? C.hi : C.accent;
    ctx.fillRect(left, 10 + i * rh, X(p.events_in_gnats) - left, rh - 4);
    if (hi.has(p.id)) { ctx.strokeStyle = C.fg; ctx.strokeRect(left, 10 + i * rh, X(p.events_in_gnats) - left, rh - 4); }
    ctx.fillStyle = C.fg;
    ctx.fillText(p.id.slice(0, 12), 4, 10 + i * rh + rh / 2 + 2);
  });
  if (GX.part.length > items.length) { ctx.fillStyle = C.muted; ctx.fillText(`Display truncation: top ${items.length} of ${GX.part.length} neurons; the table lists all.`, left, 8 + items.length * rh + 36); }
}

// ---- 5. Intervention
const IX = { result: null };
view("intervention", async (root) => {
  if (!S.source) { root.replaceChildren(heading("Intervention"), note("No source imported.")); return; }
  await loadIndex();
  await topoPage(S.source, 0, 2000);
  const t = topoCache(S.source), hidden = [...t.byIndex.values()].filter((n) => n.role === "hidden");
  const regions = [...new Set(hidden.map((n) => n.region))].sort(natural), types = [...new Set(hidden.map((n) => n.type))].sort(natural);
  const partial = t.byIndex.size < t.meta.total_neurons ? ` (from ${t.byIndex.size} of ${t.meta.total_neurons} loaded neurons)` : "";
  const ivId = textInput("", { placeholder: "automatic" });
  const selKind = selectEl([["neurons", "Neurons from the selection"], ["synapses", "Synapses from the selection"], ["region", "Region"], ["cell-type", "Cell type"], ["random", "Random hidden fraction"]], "neurons", () => syncForm());
  const region = selectEl(regions.map((r) => [r, r]), regions[0]), ctype = selectEl(types.map((r) => [r, r || "(untyped)"]), types[0]);
  const frac = numInput(0.1, { min: 0, max: 1, step: 0.01 }), rseed = numInput(1, { min: 0, step: 1 });
  const act = selectEl([["neuron-ablation", "Neuron ablation (structural mask)"], ["synapse-ablation", "Synapse ablation"], ["weight-scale", "Weight scale"], ["weight-perturb", "Weight perturbation"], ["silence", "Transient silence"], ["sham", "Sham control"]], "neuron-ablation", () => syncForm());
  const factor = numInput(2, { step: 0.1 }), sigma = numInput(0.1, { step: 0.01, min: 0 }), pseed = numInput(1, { min: 0, step: 1 }), s0 = numInput(80, { min: 0 }), s1 = numInput(120, { min: 0 });
  const split = selectEl([["development", "development"]], "development");
  const F = field;
  const fields = { region: F("Region", region, `Hidden cells only${partial}`), "cell-type": F("Cell type", ctype, `Hidden cells only${partial}`), frac: F("Fraction (0–1)", frac), rseed: F("Selection seed", rseed, "Integer; seeds above 2^53 are not exact in a browser."),
    factor: F("Factor", factor), sigma: F("Sigma", sigma), pseed: F("Perturbation seed", pseed), s0: F("Start (ms)", s0), s1: F("End (ms)", s1) };
  const preview = h("p.note", { "aria-live": "polite" }), msg = h("p.note", { "aria-live": "assertive" }), planBox = h("div"), out = h("div");
  function selection() {
    const k = selKind.value, b = S.basket;
    if (k === "neurons") return { kind: "neurons", ids: [...b.neurons] };
    if (k === "synapses") return { kind: "synapses", ids: [...b.synapses] };
    if (k === "region") return { kind: "region", region: region.value };
    if (k === "cell-type") return { kind: "cell-type", type_name: ctype.value };
    return { kind: "random", fraction: +frac.value, seed: intOf(rseed.value) };
  }
  function action() {
    const k = act.value;
    if (k === "weight-scale") return { kind: k, factor: +factor.value };
    if (k === "weight-perturb") return { kind: k, sigma: +sigma.value, seed: intOf(pseed.value) };
    if (k === "silence") return { kind: k, start_ms: +s0.value, end_ms: +s1.value };
    return { kind: k };
  }
  function syncForm() {
    const k = selKind.value, a = act.value;
    fields.region.hidden = k !== "region"; fields["cell-type"].hidden = k !== "cell-type"; fields.frac.hidden = fields.rseed.hidden = k !== "random";
    fields.factor.hidden = a !== "weight-scale"; fields.sigma.hidden = fields.pseed.hidden = a !== "weight-perturb"; fields.s0.hidden = fields.s1.hidden = a !== "silence";
    const s = selection(), ids = s.ids || (s.kind === "region" ? hidden.filter((n) => n.region === s.region).map((n) => n.id) : s.kind === "cell-type" ? hidden.filter((n) => n.type === s.type_name).map((n) => n.id) : null);
    preview.textContent = ids ? `Affected stable IDs (preview; Rust resolves authoritatively${partial}): ${ids.length ? ids.join(", ") : "none"}` : `Random selection: about ${Math.max(+frac.value > 0 ? 1 : 0, Math.round(+frac.value * hidden.length))} of ${hidden.length} hidden cells, resolved deterministically by Rust from the seed.`;
  }
  function addIv() {
    const s = selection(), a = action(), probs = [];
    if (s.ids && !s.ids.length && a.kind !== "sham") probs.push(`the selection has no ${s.kind}; add some in the Connectome view`);
    if ((a.kind === "neuron-ablation" || a.kind === "silence") && s.kind === "synapses") probs.push(`${a.kind} needs a neuron selection`);
    if (a.kind === "synapse-ablation" && s.kind !== "synapses") probs.push("synapse ablation needs a synapses selection");
    if (s.kind === "random" && !(s.fraction >= 0 && s.fraction <= 1)) probs.push("fraction must be within 0–1");
    if (a.kind === "silence" && !(a.end_ms > a.start_ms && a.start_ms >= 0)) probs.push("silence needs 0 ≤ start < end");
    for (const x of [s.seed, a.seed]) if (x === null) probs.push("seeds must be non-negative integers");
    for (const x of [a.factor, a.sigma]) if (x !== undefined && !Number.isFinite(x)) probs.push("weight parameters must be finite numbers");
    const id = ivId.value.trim() || `iv${S.plan.length + 1}-${a.kind}-${s.kind}`;
    if (S.plan.some((p) => p.id === id)) probs.push(`intervention ID ${id} is already in the plan`);
    if (probs.length) { msg.textContent = `Not added: ${probs.join("; ")}.`; return; }
    changePlan(() => S.plan.push({ id, selection: s, action: a }));
    ivId.value = "";
    msg.textContent = `Added ${id}. The selection stays as it is; clear or undo it separately.`;
  }
  function changePlan(fn) { S.planHistory.push(JSON.stringify(S.plan)); fn(); fillPlan(); }
  function fillPlan() {
    const plan = { schema: "aarnn-explorer-plan-v1", source: S.source, split: split.value, interventions: S.plan };
    planBox.replaceChildren(
      table("Draft plan (uncommitted)", [["ID", (x) => h("code", { text: x.id })], ["Selection", (x) => selText(x.selection)], ["Action", (x) => actText(x.action)], ["Remove", (x) => button("Remove", () => changePlan(() => { S.plan = S.plan.filter((y) => y !== x); }), { cls: "small" })]], S.plan, { empty: "No interventions yet." }),
      h("div.row", null, button("Undo last plan change", () => { const p = S.planHistory.pop(); if (p) { S.plan = JSON.parse(p); fillPlan(); announce("Last plan change undone"); } }, { disabled: !S.planHistory.length }),
        button("Run experiment", () => {
          out.replaceChildren(note("Submitted; the intact reference and a sham control run first.", "loading"));
          submitJob({ kind: "experiment", plan }, `experiment (${S.plan.length} interventions)`, async (j) => {
            if (j.state !== "succeeded") { out.replaceChildren(jobResult(j)); return; }
            const rec = j.result.object ? await object(j.result.object).catch(() => ({})) : {};
            IX.result = { ...rec, ...j.result };
            out.replaceChildren(experimentView(IX.result));
          }).catch((e) => out.replaceChildren(errBox(e)));
        }, { cls: "primary", disabled: !S.plan.length })),
      h("details", null, h("summary", { text: "Plan JSON as submitted (stable string IDs)" }), h("pre", { text: JSON.stringify(plan, null, 2) })));
  }
  const past = entries("results").filter(([n]) => n.startsWith("experiment-"));
  const pastSel = selectEl([["", "Choose an experiment"], ...past.map(([n]) => [n, n])], "", async (v) => { if (!v) return; try { out.replaceChildren(experimentView(await object(S.index.results[v]))); } catch (e) { out.replaceChildren(errBox(e)); } });
  root.replaceChildren(heading("Intervention", "intervention"),
    note("Interventions run on isolated copies of the immutable source at a declared cold start (tick 0). A display selection never touches a running brain."),
    h("div.split", null,
      h("section.card", null, h("h3", { text: "Add an intervention" }), field("Intervention ID", ivId), field("Selection", selKind), fields.region, fields["cell-type"], fields.frac, fields.rseed,
        field("Action", act), fields.factor, fields.sigma, fields.pseed, fields.s0, fields.s1, preview, h("div.row", null, button("Add to plan", addIv, { cls: "primary" })), msg, field("Split", split, "Experiments use the development split; locked validation is reserved for Validate.")),
      basketBox()),
    planBox, h("div.toolbar", null, field("Earlier experiment results", pastSel)), out);
  syncForm();
  fillPlan();
  if (IX.result) out.replaceChildren(experimentView(IX.result));
});
const intOf = (v) => /^\d+$/.test(String(v).trim()) && Number.isSafeInteger(+v) ? +v : null;
const selText = (s) => s.kind === "neurons" || s.kind === "synapses" ? `${s.kind}: ${s.ids.join(", ") || "none"}` : s.kind === "region" ? `region ${s.region}` : s.kind === "cell-type" ? `cell type ${s.type_name}` : `random ${s.fraction} (seed ${s.seed})`;
const actText = (a) => a.kind + (a.factor != null ? ` ×${a.factor}` : "") + (a.sigma != null ? ` σ=${a.sigma}, seed ${a.seed}` : "") + (a.start_ms != null ? ` ${a.start_ms}–${a.end_ms} ms` : "");
function experimentView(r) {
  const rows = r.rows || [];
  return h("section.card", null, h("h3", null, "Experiment result ", r.label ? h("code", { text: r.label }) : null),
    dl([["Object", r.object ? code(r.object) : null], ["Source", r.source], ["Split", r.split], ["Intact reference", badge(r.intact_passed)], ["Sham matches intact", badge(r.sham_matches_intact ? "passed: identical measures" : "failed: sham differs (controls are suspect)")], ["Source unchanged", badge(r.source_unchanged ? "passed: hash identical before and after" : "failed: the source changed")]]),
    note("Development-split behavioural outcomes under each intervention. This is exploratory evidence, not a validation, a resilience claim or a biological finding."),
    table("Intervention outcomes", [["Intervention", (x) => h("code", { text: x.intervention?.id })], ["Selection", (x) => selText(x.intervention.selection)], ["Action", (x) => actText(x.intervention.action)],
      ["Applied", (x) => x.error ? badge("error") : (x.applied || []).map((a) => a.classification).join(", ")], ["Affected neurons", (x) => (x.applied || []).flatMap((a) => a.neurons).join(", ") || "none"], ["Affected synapses", (x) => (x.applied || []).flatMap((a) => a.synapses).length],
      ["Passed", (x) => x.error ? errBox(x.error) : badge(x.passed)], ["Quiescent", (x) => x.quiescent], ["Failed tasks", (x) => x.failed_tasks], ["Neurons", (x) => x.cost?.neurons], ["Synapses", (x) => x.cost?.synapses],
      ["State bytes (estimate)", (x) => x.cost?.state_bytes], ["Step p50 (measured)", (x) => fmtNs(x.cost?.latency_ns_p50)]], rows, { rowClass: (x) => (x.error || !x.passed ? "differs" : null) }));
}

// ---- 6. Comparison
const CMP = { a: null, b: null };
function verdictsOf(o) {
  if (o.schema === "aarnn-explorer-validation-v1") return [["candidate", "Candidate (locked split)", o.candidate_verdict, o.cost?.candidate], ["intact", "Intact (same split)", o.intact, o.cost?.intact]];
  if (o.kind === "experiment") return (o.rows || []).map((r) => [r.intervention.id, `Intervention ${r.intervention.id}`, { split: o.split, passed: r.passed, quiescent: r.quiescent, derived: r.failed_tasks || [], error: r.error }, r.cost]);
  if (o.verdict) return [["verdict", `${o.kind || "result"} ${o.label || ""}`, o.verdict, o.cost]];
  return [];
}
view("comparison", async (root) => {
  await loadIndex();
  const res = entries("results");
  if (res.length < 1) { root.replaceChildren(heading("Comparison", "comparison"), note("No result objects yet.")); return; }
  const out = h("div");
  const side = (k, def, vi) => {
    const vs = h("select", { onchange: () => compare() });
    const os = selectEl(res.map(([n]) => [n, n]), CMP[k]?.name || def, () => pick());
    async function pick() {
      try {
        const o = await object(S.index.results[os.value]), list = verdictsOf(o);
        CMP[k] = { name: os.value, o, list };
        vs.replaceChildren(...list.map(([id, t], i) => h("option", { value: String(i), text: t, selected: i === (os.value === def ? vi : 0) })));
        if (!list.length) vs.append(h("option", { value: "", text: "No verdict in this object" }));
        compare();
      } catch (e) { out.replaceChildren(errBox(e)); }
    }
    CMP[k + "sel"] = vs;
    return [h("fieldset.side", null, h("legend", { text: `Side ${k.toUpperCase()}` }), field("Result object", os), field("Verdict", vs)), pick];
  };
  // Default: intact against candidate inside one validation object (same split).
  const val = res.find(([n]) => n.startsWith("validation-"))?.[0];
  const base = res.find(([n]) => !n.startsWith("experiment-") && !n.startsWith("validation-"))?.[0] || res[0][0];
  const [fa, pa] = val ? side("a", val, 1) : side("a", base, 0), [fb, pb] = side("b", val || res[res.length - 1][0], 0);
  root.replaceChildren(heading("Comparison", "comparison"), note("Pick two result or validation objects. Rows that differ are marked “differs”. Experiment rows record failed tasks only, so their per-task verdicts are derived and their mean measures are absent."), h("div.split", null, fa, fb), out);
  await pa();
  await pb();
  function compare() {
    if (!CMP.a || !CMP.b) return;
    const A = CMP.a.list[+CMP.asel.value], B = CMP.b.list[+CMP.bsel.value];
    if (!A || !B) { out.replaceChildren(note("Choose a verdict on each side.")); return; }
    const va = A[2], vb = B[2], names = [...new Set([...(va.tasks || []).map((t) => t.task), ...(vb.tasks || []).map((t) => t.task), ...(va.derived || []), ...(vb.derived || [])])];
    const known = (v) => v.tasks ? v.tasks.map((t) => t.task) : null, all = known(va) || known(vb) || names;
    const task = (v, n) => v.tasks ? v.tasks.find((t) => t.task === n) : v.error ? null : { task: n, passed: !v.derived.includes(n), derived: true };
    const show = (t) => t ? h("span", null, badge(t.passed), t.derived ? " (derived)" : ` ${t.successes}/${t.trials}`) : "–";
    const rows = all.map((n) => [n, task(va, n), task(vb, n)]);
    const meas = [];
    for (const [n, ta, tb] of rows) for (const m of new Set([...Object.keys(ta?.mean_measures || {}), ...Object.keys(tb?.mean_measures || {})])) meas.push([n, m, ta?.mean_measures?.[m], tb?.mean_measures?.[m]]);
    const sameSplit = va.split === vb.split;
    const ca = A[3] || {}, cb = B[3] || {};
    out.replaceChildren(
      sameSplit ? note(`Both sides use split ${va.split}: the same declared stimulus and environment definition.`) : errBox(`Different splits (${va.split} vs ${vb.split}): these sides do not share a stimulus definition, so differences are not attributable to the circuit alone.`),
      dl([["A", `${CMP.a.name}: ${A[1]}`], ["B", `${CMP.b.name}: ${B[1]}`], ["Suite passed", h("span", null, "A ", badge(va.passed), " · B ", badge(vb.passed))], ["Quiescent", `A ${cell(va.quiescent)} · B ${cell(vb.quiescent)}`]]),
      table("Per-task verdicts", [["Task", (r) => r[0]], ["A", (r) => show(r[1])], ["B", (r) => show(r[2])], ["Difference", (r) => r[1]?.passed !== r[2]?.passed ? "differs" : "same"]], rows, { rowClass: (r) => r[1]?.passed !== r[2]?.passed ? "differs" : null }),
      table("Mean measures", [["Task", (r) => r[0]], ["Measure", (r) => r[1]], ["A", (r) => r[2]], ["B", (r) => r[3]], ["B − A", (r) => r[2] != null && r[3] != null ? r[3] - r[2] : "–"], ["Difference", (r) => r[2] === r[3] ? "same" : "differs"]], meas, { rowClass: (r) => r[2] !== r[3] ? "differs" : null, empty: "No mean measures on one or both sides." }),
      table("Cost", [["Metric", (r) => r[0]], ["A", (r) => r[1]], ["B", (r) => r[2]], ["Basis", (r) => r[3]]], [["Neurons", ca.neurons, cb.neurons, "exact count"], ["Synapses", ca.synapses, cb.synapses, "exact count"], ["State bytes", ca.state_bytes, cb.state_bytes, "estimated (proxy)"], ["Memory bytes", ca.memory_bytes, cb.memory_bytes, "estimated (proxy)"], ["Step p50", fmtNs(ca.latency_ns_p50), fmtNs(cb.latency_ns_p50), "measured on this host"]], { rowClass: (r) => r[1] !== r[2] ? "differs" : null }));
  }
});

// ---- 7. Resilience
const RX = { strip: null, faults: [] };
view("resilience", async (root) => {
  await loadIndex();
  const opts = [...entries("results").filter(([n]) => n.startsWith("validation-")).map(([n]) => [`r:${n}`, `${n} (locked validation)`]), ...entries("candidates").map(([n]) => [`c:${n}`, `${n} (development resilience at selection)`])];
  if (!opts.length) { root.replaceChildren(heading("Resilience", "resilience"), note("No validation or candidate objects yet. Run Optimisation, then Validate in the Export view.")); return; }
  const out = h("div");
  const sel = selectEl(opts, opts[0][0], () => load());
  root.replaceChildren(heading("Resilience", "resilience"), h("div.toolbar", null, field("Evidence object", sel)),
    note("Fault, recovery and evidence settings come from the server-side profile (TOML). To change them, pass a different profile path when validating in the Export view."), out);
  async function load() {
    try {
      const [k, n] = [sel.value.slice(0, 1), sel.value.slice(2)];
      const o = await object(k === "r" ? S.index.results[n] : S.index.candidates[n]);
      const r = k === "r" ? o.resilience : o.development_resilience;
      out.replaceChildren(...(k === "r" ? [claimsPanel(o.verdicts, `Split ${o.split}${o.locked ? " (locked)" : " (not the locked split)"}; profile ${o.profile}.`)] : [note("Development-split resilience checked while selecting the candidate: selection evidence, not a locked validation.")]), resilienceView(r));
      drawStrip();
    } catch (e) { out.replaceChildren(errBox(e)); }
  }
  await load();
}, () => drawStrip());
function resilienceView(r) {
  if (!r || r.verdict == null) return note("No resilience record in this object.");
  RX.faults = Array.isArray(r.single_faults) ? r.single_faults : [];
  const claims = r.claims || [], failOnly = h("input", { type: "checkbox", checked: false });
  const ft = h("div"), fill = () => ft.replaceChildren(table(`Single-neuron faults (${RX.faults.filter((f) => !f.passed).length} failing of ${RX.faults.length})`, [["Neuron", (f) => h("code", { text: f.neuron })], ["Survived", (f) => badge(f.passed)], ["Failed tasks", (f) => f.failed_tasks]], failOnly.checked ? RX.faults.filter((f) => !f.passed) : RX.faults, { rowClass: (f) => f.passed ? null : "differs", empty: "No single-fault cases." }));
  failOnly.addEventListener("change", fill);
  fill();
  RX.strip = h("canvas.strip", { role: "img", "aria-label": "Single-fault outcome per neuron: filled squares survived, crosses failed. The table is the tabular alternative." });
  RX.ci = h("canvas.strip", { role: "img", "aria-label": "Confidence bounds of sampled claims against their required success probability; the claims table lists the values." });
  RX.claims = claims;
  return h("section.card", null, h("h3", { text: `Resilience: ${r.verdict}` }),
    dl([["Verdict", badge(r.verdict)], ["Profile", r.profile], ["Family confidence", r.family_confidence], ...Object.entries(r.expanded || {}).map(([k, v]) => [`Profile: ${k}`, v])]),
    table("Claims", [["Claim", (c) => c.claim], ["Verdict", (c) => badge(c.verdict)], ["Evidence", (c) => c.tested != null ? `${c.survived}/${c.tested} survived` : `${c.successes}/${c.trials} trials passed`], ["Lower bound", (c) => c.lower_bound], ["Upper bound", (c) => c.upper_bound],
      ["Per-claim confidence", (c) => c.per_claim_confidence], ["Trials needed if no failures", (c) => c.trials_needed_if_no_failures], ["Method / scope", (c) => c.method || c.scope]], claims, { empty: "No claims were required by this profile." }),
    claims.some((c) => c.lower_bound != null) ? RX.ci : null,
    ...claims.filter((c) => c.failures?.length).map((c) => table(`Failing samples: ${c.claim}`, [["Removed neurons", (f) => f.selection], ["Failed tasks", (f) => f.failed], ["Job", (f) => f.job]], c.failures)),
    RX.faults.length ? RX.strip : null, field("Show failing cases only", failOnly), ft);
}
function drawStrip() {
  const C = colours();
  if (RX.strip?.isConnected && RX.strip.clientWidth) {
    const f = RX.faults, cw = 14, per = Math.max(1, Math.floor((RX.strip.clientWidth - 8) / cw)), { ctx } = prep(RX.strip, 10 + Math.ceil(f.length / per) * cw + 6);
    f.forEach((x, i) => {
      const px = 4 + (i % per) * cw, py = 6 + Math.floor(i / per) * cw;
      ctx.fillStyle = ctx.strokeStyle = x.passed ? C.ok : C.bad;
      ctx.lineWidth = 2;
      if (x.passed) ctx.fillRect(px, py, cw - 3, cw - 3); else marker(ctx, "cross", px + (cw - 3) / 2, py + (cw - 3) / 2, (cw - 5) / 2);
    });
  }
  if (RX.ci?.isConnected && RX.ci.clientWidth) {
    const cs = RX.claims.map((c, i) => ({ ...c, n: i + 1 })).filter((c) => c.lower_bound != null), { ctx, w } = prep(RX.ci, 44 + cs.length * 32), box = { l: 30, r: w - 20, t: 6, b: 6 + cs.length * 32 };
    const { X } = axes(ctx, C, box, [0, 1], null, "success probability");
    cs.forEach((c, i) => {
      const y = 26 + i * 32, need = />=\s*([\d.]+)/.exec(c.claim);
      ctx.strokeStyle = c.verdict === "passed" ? C.ok : c.verdict === "failed" ? C.bad : C.warn; ctx.lineWidth = 4;
      ctx.beginPath(); ctx.moveTo(X(c.lower_bound), y); ctx.lineTo(X(c.upper_bound), y); ctx.stroke();
      if (need && need[1]) { ctx.strokeStyle = C.fg; ctx.lineWidth = 2; ctx.beginPath(); ctx.moveTo(X(+need[1]), y - 8); ctx.lineTo(X(+need[1]), y + 8); ctx.stroke(); }
      ctx.fillStyle = C.fg; ctx.fillText(`Claim ${c.n}: ${c.verdict}; bounds ${fmt(c.lower_bound)}–${fmt(c.upper_bound)}${need ? `; required ${need[1]} (vertical mark)` : ""}`, box.l + 4, y - 9);
    });
  }
}

// ---- 8. Optimisation
const OX = { rep: null, cv: null, pts: [] };
view("optimisation", async (root) => {
  await loadIndex();
  const reps = entries("reports").filter(([n]) => n.startsWith("optimisation-"));
  const selRule = selectEl([["min-state-bytes", "min-state-bytes"], ["min-neurons", "min-neurons"], ["min-synapses", "min-synapses"]], "min-state-bytes");
  const cfg = textInput("", { placeholder: "server default profile" }), an = selectEl(entries("analyses").length ? entries("analyses").map(([n]) => [n, n]) : [["", "none (Rust falls back to intact-development-gnat if it appears)"]], "intact-development-gnat");
  const out = h("div"), status = h("div");
  const repSel = selectEl(reps.length ? reps.map(([n]) => [n, n]) : [["", "No optimisation reports yet"]], OX.name || reps[reps.length - 1]?.[0] || "", (v) => showReport(v));
  root.replaceChildren(heading("Optimisation", "optimisation"),
    h("section.card", null, h("h3", { text: "Start a reduction search" }),
      note("The search re-evaluates every proposal in a worker. Pause stops admission and terminates running workers; completed worker results are kept, so Resume (the same submission) reuses them. Cancel uses the same server call without offering Resume."),
      h("div.row", null, field("Selection rule", selRule), field("Analysis for importance", an), field("Profile config path (optional)", cfg, "Server-side TOML path, e.g. configs/explorer/resilient.toml")),
      button("Start optimisation", () => {
        const body = { kind: "optimise", source: S.source, select: selRule.value };
        if (an.value) body.analysis = an.value;
        if (cfg.value.trim()) body.config = cfg.value.trim();
        submitJob(body, `optimise ${selRule.value}`, (j) => {
          status.replaceChildren(jobResult(j));
          if (j.state === "succeeded" && j.result?.report) build("optimisation");
        }).catch((e) => status.replaceChildren(errBox(e)));
      }, { cls: "primary" }), status),
    h("div.toolbar", null, field("Optimisation report", repSel)), out);
  OX.out = out;
  if (repSel.value) await showReport(repSel.value);
}, () => drawScatter());
async function showReport(name) {
  OX.name = name;
  const out = OX.out;
  try {
    const r = OX.rep = await object(S.index.reports[name]);
    const cand = r.candidate_hash ? await object(r.candidate_hash).catch(() => null) : null;
    const key = (p) => JSON.stringify([p.removed_neurons || [], p.removed_synapses || []]);
    const front = new Set((r.frontier || []).map(key)), chosen = cand ? key(cand) : null;
    const props = (r.proposals || []).map((p, i) => ({ ...p, i, front: front.has(key(p)), chosen: key(p) === chosen }));
    OX.pts = props;
    OX.cv = h("canvas.scatter", { role: "img", "aria-label": "Scatter of proposals: neurons against state bytes. Diamonds are Pareto frontier, circles passed, crosses failed, the ringed point is the selected candidate. The tables list every point." });
    const ic = r.intact_cost || {}, sc = r.selected_cost || {};
    out.replaceChildren(
      h("section.card", null, h("h3", { text: `Report ${name}` }),
        dl([["Result", badge(r.result)], ["Reason", r.reason], ["Candidate", r.candidate ? h("span", null, h("code", { text: r.candidate }), " ", code(r.candidate_hash)) : "none"], ["Proposals evaluated", `${r.proposals_evaluated} of budget ${r.budget}`],
          ["Inert cells removed first", r.inert_removed_first], ["Inert notes", r.inert_notes], ["Neurons", `${ic.neurons} → ${sc.neurons}`], ["Synapses", `${ic.synapses} → ${sc.synapses}`], ["State bytes (estimate)", `${fmtBytes(ic.state_bytes)} → ${fmtBytes(sc.state_bytes)}`], ["Step p50 (measured)", `${fmtNs(ic.latency_ns_p50)} → ${fmtNs(sc.latency_ns_p50)}`]]),
        note("Development-split search. Selection is not validation: run Validate on the locked split in the Export view.")),
      OX.cv, legend([["front", "Pareto frontier (diamond)"], ["ok", "Passed (circle)"], ["bad", "Failed constraint (cross)"], ["sel", "Selected candidate (ring)"]]),
      note("Proposals with identical costs share a point; the proposal log lists every one."),
      table("Pareto frontier", [["Removed neurons", (p) => p.removed_neurons], ["Removed synapses", (p) => p.removed_synapses], ["Neurons", (p) => p.cost.neurons], ["Synapses", (p) => p.cost.synapses], ["State bytes (estimate)", (p) => p.cost.state_bytes], ["Memory bytes (estimate)", (p) => p.cost.memory_bytes], ["Step p50 (measured)", (p) => fmtNs(p.cost.latency_ns_p50)]], r.frontier || []),
      table("Proposal log", [["#", (p) => p.i], ["Round", (p) => p.round], ["Removed neurons", (p) => p.removed_neurons], ["Removed synapses", (p) => p.removed_synapses], ["Passed", (p) => badge(p.passed)], ["Failed constraints (tasks)", (p) => p.failed_tasks],
        ["Neurons", (p) => p.cost.neurons], ["State bytes", (p) => p.cost.state_bytes], ["Step p50", (p) => fmtNs(p.cost.latency_ns_p50)], ["Frontier", (p) => p.front], ["Selected", (p) => p.chosen], ["Worker job", (p) => h("code", { text: p.job })]], props, { rowClass: (p) => p.passed ? null : "differs" }),
      table("Hypothesis order (single-ablation screen plus GNAT participation; hypotheses, not proofs)", [["Neuron", (x) => h("code", { text: x.id })], ["Importance", (x) => x.importance], ["Single ablation failed", (x) => x.single_ablation_failed]], r.hypothesis_order || []),
      table("Resilience checks during selection", [["Removed neurons", (x) => x.removed], ["Resilience", (x) => badge(x.resilience)], ["Single faults survived", (x) => badge(x.single_faults_ok)]], r.resilience_checked || [], { empty: "No resilience checks were required by this profile." }));
    drawScatter();
  } catch (e) { out.replaceChildren(errBox(e)); }
}
function drawScatter() {
  const cv = OX.cv;
  if (!cv || !cv.isConnected || !cv.clientWidth || !OX.pts.length) return;
  const { ctx, w, h: hh } = prep(cv, 340), C = colours(), P = OX.pts;
  const xs = P.map((p) => p.cost.neurons), ys = P.map((p) => p.cost.state_bytes), pad = (a, b) => [a - Math.max(1, (b - a) * 0.08), b + Math.max(1, (b - a) * 0.08)];
  const { X, Y } = axes(ctx, C, { l: 64, r: w - 14, t: 10, b: hh - 40 }, pad(Math.min(...xs), Math.max(...xs)), pad(Math.min(...ys), Math.max(...ys)), "neurons", "state bytes (estimate)");
  for (const p of [...P].sort((a, b) => a.front - b.front)) {
    const x = X(p.cost.neurons), y = Y(p.cost.state_bytes);
    ctx.lineWidth = 1.5;
    ctx.strokeStyle = ctx.fillStyle = p.front ? C.accent : p.passed ? C.ok : C.bad;
    marker(ctx, p.front ? "diamond" : p.passed ? "circle" : "cross", x, y, 4);
    if (p.chosen) { ctx.strokeStyle = C.fg; ctx.lineWidth = 2; marker(ctx, "circle", x, y, 9); }
  }
}

// ---- 9. Export and evidence
view("export", async (root) => {
  await loadIndex();
  const sel = S.index.aliases?.selected, cands = entries("candidates"), selName = cands.find(([, hh]) => hh === sel)?.[0];
  const opts = [...(sel ? [["selected", `selected (alias → ${selName || `${sel.slice(0, 12)}…`})`]] : []), ...cands.map(([n]) => [n, n])];
  if (!opts.length) { root.replaceChildren(heading("Export and evidence", "export"), note("No candidate yet. Run Optimisation first.")); return; }
  const cSel = selectEl(opts, opts[0][0], () => show()), split = selectEl([["locked-validation", "locked-validation"], ["biological-challenge", "biological-challenge"]], "locked-validation");
  const cfg = textInput("", { placeholder: "server default profile" }), expl = h("input", { type: "checkbox" }), info = h("div"), jobs = h("div");
  root.replaceChildren(heading("Export and evidence", "export"), h("div.toolbar", null, field("Candidate", cSel)),
    h("section.card", null, h("h3", { text: "Validate and export" }),
      h("div.row", null, field("Split", split), field("Profile config path (optional)", cfg, "Server-side TOML path; resilience settings come from it."),
        button("Validate candidate", () => {
          const body = { kind: "validate", candidate: cSel.value, split: split.value };
          if (cfg.value.trim()) body.config = cfg.value.trim();
          submitJob(body, `validate ${cSel.value} on ${split.value}`, (j) => { jobs.replaceChildren(jobResult(j)); show(); }).catch((e) => jobs.replaceChildren(errBox(e)));
        }, { cls: "primary" })),
      h("div.row", null, field("Allow an unvalidated export (labelled exploratory-unvalidated)", expl),
        button("Export bundle", () => submitJob({ kind: "export", candidate: cSel.value, exploratory: expl.checked }, `export ${cSel.value}`, (j) => { jobs.replaceChildren(j.state === "succeeded" ? exportView(j.result) : jobResult(j)); if (j.state === "succeeded") show(); }).catch((e) => jobs.replaceChildren(errBox(e))))), jobs),
    info);
  async function show() {
    try {
      await loadIndex();
      const hash = cSel.value === "selected" ? S.index.aliases.selected : S.index.candidates[cSel.value], c = await object(hash);
      const vals = entries("results").filter(([n]) => n.startsWith(`validation-${hash.slice(0, 12)}`));
      const vObjs = await Promise.all(vals.map(([n, hh]) => object(hh).then((o) => [n, o])));
      info.replaceChildren(h("section.card", null, h("h3", null, "Candidate ", code(hash)),
        dl([["Source", c.source], ["Profile", c.profile], ["Selection rule", c.selection_rule], ["Classification", c.classification], ["Removed neurons", c.removed_neurons], ["Removed synapses", c.removed_synapses], ["Development pass", badge(c.development?.passed)], ["Validated flag at selection", c.validated], ["Lowered snapshot", code(c.snapshot_object)]])),
      ...(vObjs.length ? vObjs.map(([n, v]) => h("section.card", null, h("h3", { text: n }), claimsPanel(v.verdicts, `Split ${v.split}${v.locked ? " (locked)" : ""}; profile ${v.profile}.`), costTable(v.cost?.intact, v.cost?.candidate, null, S.costClass)))
        : [claimsPanel(null, "No validation of this candidate yet: every claim is not assessed."), costTable(c.intact_cost, c.development?.cost, "development split", S.costClass)]));
    } catch (e) { info.replaceChildren(errBox(e)); }
  }
  await show();
});
// Measured/estimated labels come from the bundle's cost.json (export result `cost`).
function costTable(a, b, ctx, cls) {
  a = a || {}; b = b || {};
  const d = (x, y) => x != null && y != null ? `${y - x >= 0 ? "+" : ""}${fmt(y - x)}${x ? ` (${(((y - x) / x) * 100).toFixed(1)} %)` : ""}` : "–";
  const basis = (k) => {
    if (["neurons", "hidden", "synapses"].includes(k)) return h("span.badge.na", { text: "structural count" });
    if (!cls) return h("span.badge.na", { text: "classified in the bundle's cost.json: export to see it" });
    const m = (cls.measured || []).find((x) => x.startsWith(k)), e = (cls.estimated || []).find((x) => x.startsWith(k));
    return m ? h("span.badge.ok", { text: `measured: ${m}` }) : e ? h("span.badge.warn", { text: `estimated: ${e}` }) : h("span.badge.na", { text: "not classified by cost.json" });
  };
  const rows = [["Neurons", "neurons"], ["Hidden cells", "hidden"], ["Synapses", "synapses"], ["State bytes", "state_bytes"], ["Memory bytes", "memory_bytes"], ["Step latency p50 (ns)", "latency_ns_p50"]].map(([l, k]) => [l, a[k], b[k], basis(k)]);
  for (const x of (cls && cls.not_available) || []) rows.push([x, null, null, h("span.badge.na", { text: "not available" })]);
  return h("div", null, table(`Hardware cost: intact vs candidate${ctx ? ` (${ctx})` : ""}`, [["Metric", (r) => r[0]], ["Intact", (r) => r[1]], ["Candidate", (r) => r[2]], ["Change", (r) => d(r[1], r[2])], ["Basis", (r) => r[3]]], rows),
    note(`Cost basis reported by Rust: ${b.cost_basis || a.cost_basis || "–"}.${cls ? " Measured and estimated labels are from the exported bundle's cost.json." : ""}`));
}
function exportView(r) {
  if (r.cost) S.costClass = r.cost;
  return h("section.card", null, h("h3", { text: "Exported bundle" }),
    dl([["Status", badge(r.status)], ["Bundle", r.reused ? "existing bundle reused (same candidate and profile)" : "newly written"], ["Bundle directory (on the server host)", code(r.bundle)], ["Candidate", code(r.candidate)], ["Files", r.files || "see the bundle directory"]]),
    r.cost ? costTable(r.cost.intact, r.cost.candidate, "bundle cost.json", r.cost) : null,
    note(`The API does not serve bundle files for download; open the directory above. Verify it in a clean process with: aarnn-explorer verify-bundle --bundle ${r.bundle}`));
}

// ---- 10. Resources and status
view("status", async (root) => {
  const [st, jl] = await Promise.all([api("/status"), api("/jobs"), loadIndex()]);
  const obj2rows = (o) => Object.entries(o || {});
  const kv = (title, o) => table(title, [["Item", (x) => x[0]], ["Status", (x) => typeof x[1] === "string" ? badge(x[1]) : x[1]]], obj2rows(o));
  const tc = h("div");
  const ans = entries("analyses");
  const anSel = selectEl(ans.map(([n]) => [n, n]), ans[0]?.[0]);
  root.replaceChildren(heading("Resources and status", "status"),
    h("section.card", null, h("h3", { text: "Job queue (this server session)" }),
      table("Jobs", [["Job", (j) => h("code", { text: j.job })], ["Kind", (j) => j.kind], ["State", (j) => badge(j.state)], ["Events", (j) => j.events], ["Error", (j) => j.error], ["Parameters", (j) => h("details", null, h("summary", { text: Object.keys(j.params || {}).join(", ") }), h("pre", { text: JSON.stringify(j.params, null, 2) }))],
        ["Action", (j) => TERMINAL.has(j.state) ? "–" : button("Cancel", () => cancelJob(trackJob(j.job, j.kind, `${j.kind} ${j.job}`, j.params), false), { cls: "small" })]], jl.jobs || [], { empty: "No jobs submitted in this session." }),
      note("Job success is separate from scientific outcome: a succeeded experiment may show that a candidate fails.")),
    h("section.card", null, h("h3", { text: "Resource enforcement and budgets" }), kv("Enforcement", st.resource_enforcement),
      st.budgets ? table(`Budgets of the server's default profile (${st.budgets.profile})`, [["Budget", (x) => x[0]], ["Value", (x) => x[1]]], Object.entries(st.budgets.resources || {})) : note("This server reports no budgets."),
      note("A job submitted with a profile config path runs under that profile's budgets instead."), dl([["API page bound", st.api?.max_page], ["Features", JSON.stringify(st.features)], ["Target", `${st.target?.arch} / ${st.target?.os}`]])),
    h("section.card", null, h("h3", { text: "Execution backends and unsupported capabilities" }), kv("Backends", st.backends), kv("Closed-loop environments", st.closed_loop), kv("Build requirements", st.build_requirements), h("h4", { text: "Data schemas" }), ul(st.data_schemas)),
    h("section.card", null, h("h3", { text: "Trace completeness" }), ans.length ? h("div.row", null, field("Analysis", anSel), button("Check every trace manifest", () => checkTraces(anSel.value, tc))) : note("No analysis yet."), tc));
});
async function checkTraces(name, box) {
  try {
    const an = await object(S.index.analyses[name]), keys = Object.keys(an.traces || {}).sort(natural), rows = [];
    for (const k of keys) {
      box.replaceChildren(note(`Reading manifest ${rows.length + 1} of ${keys.length}…`, "loading"));
      const m = await object(an.traces[k]);
      rows.push([k, m]);
    }
    box.replaceChildren(note(`${rows.filter((r) => r[1].complete).length} of ${rows.length} traces are complete.`),
      table("Trace manifests", [["Trace", (r) => r[0]], ["Complete", (r) => badge(r[1].complete ? "complete" : "incomplete")], ["Ticks", (r) => r[1].ticks], ["Gaps", (r) => (r[1].gaps || []).length], ["Excluded events", (r) => r[1].excluded_events],
        ["Coverage", (r) => Object.entries(r[1].coverage || {}).map(([s, c]) => `${s}: ${c}`).join("; ")]], rows));
  } catch (e) { box.replaceChildren(errBox(e)); }
}

// ---------------------------------------------------------------- start
function initTabs() {
  const tabs = [...document.querySelectorAll('[role="tab"]')];
  for (const t of tabs) {
    t.addEventListener("click", () => showTab(t.dataset.view));
    t.addEventListener("keydown", (e) => {
      const i = tabs.indexOf(t), n = tabs.length;
      const to = { ArrowRight: (i + 1) % n, ArrowLeft: (i - 1 + n) % n, Home: 0, End: n - 1 }[e.key];
      if (to !== undefined) { e.preventDefault(); showTab(tabs[to].dataset.view, true); }
    });
  }
}
async function start() {
  initTabs();
  $("#global-source").addEventListener("change", (e) => { S.source = e.target.value; S.selected = null; CX.offset = 0; S.built.clear(); showTab(S.active); announce(`Source ${S.source} chosen`); });
  let rt = null;
  window.addEventListener("resize", () => { clearTimeout(rt); rt = setTimeout(() => VIEWS[S.active]?.onShow?.(), 150); });
  matchMedia("(prefers-color-scheme: dark)").addEventListener?.("change", () => VIEWS[S.active]?.onShow?.());
  try {
    const [src] = await Promise.all([api("/sources"), loadIndex()]);
    S.sources = src.sources;
    fillSourcePicker();
    const jl = await api("/jobs");
    for (const j of jl.jobs || []) if (!TERMINAL.has(j.state)) trackJob(j.job, j.kind, `${j.kind} (already running)`, j.params);
  } catch (e) { $("#main").prepend(errBox(e)); }
  showTab(location.hash.slice(1) || "project");
}
document.addEventListener("DOMContentLoaded", start);
