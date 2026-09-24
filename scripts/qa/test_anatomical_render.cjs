/* Executes the production canvas functions against the shared Rust fixture.
 * Chrome is optional only when --browser was not requested. No live brain. */
const fs = require('node:fs');
const path = require('node:path');
const vm = require('node:vm');
const assert = require('node:assert/strict');
const { spawnSync } = require('node:child_process');
const root = path.resolve(__dirname, '../..');
const source = fs.readFileSync(path.join(root, 'web_ui/app.js'), 'utf8');
function extract(name) {
  const start = source.indexOf(`function ${name}(`);
  assert.ok(start >= 0, name);
  const end = source.indexOf('\nfunction ', start + 1);
  return source.slice(start, end < 0 ? source.length : end);
}
const functions = ['displayIdKey', 'stableNeuronColour', 'buildContractGraph', 'rotate',
  'anatomicalMembraneHull', 'drawTubePolygon', 'drawNodes', 'drawEarlyNodes', 'drawNetwork', 'rebuildGraph'].map(extract).join('\n');
const fixture = JSON.parse(fs.readFileSync(path.join(root, 'qa/fixtures/morphology/anatomical-stability.json')));
const context = { state: { snapshot: { display_snapshots: { anatomical: fixture } }, render: { layout: 'aarnn' } }, drawNetwork() {}, console };
vm.createContext(context);
vm.runInContext(functions + '\ndrawNetwork = () => {}; function buildGraph(snapshot, layout) { return buildContractGraph(snapshot, layout); }', context);
assert.notEqual(vm.runInContext("displayIdKey({value:'2305843013508661249',generation:1})", context), vm.runInContext("displayIdKey({value:'2305843013508661250',generation:1})", context));
assert.equal(vm.runInContext("displayIdKey({value:2305843013508661249,generation:1})", context), '', 'unsafe legacy numeric identity must be rejected');
vm.runInContext(`
rebuildGraph();
if (state.graph.nodes.hidden[0].length !== 2) throw Error('fixture soma count');
for (let sequence = 2; sequence < 100; sequence++) {
  state.snapshot.display_snapshots.anatomical.sequence = sequence;
  state.snapshot.display_snapshots.anatomical.coverage.complete = sequence % 2 === 0;
  state.snapshot.display_snapshots.anatomical.coverage.truncated = sequence % 2 !== 0;
  rebuildGraph();
  if (state.graph.displayContract.sequence !== sequence) throw Error('bounded snapshot frozen');
}
const paths = state.graph.edges.filter(e => e.kind === 'axon' || e.kind === 'dendrite');
if (paths.length !== 10) throw Error('missing anatomy');
if (paths[0].points[0].x !== paths[0].from.x) throw Error('disconnected projection');
state.graph = { displayContract: { mode: 'SyntheticColumns' } };
state.snapshot.display_snapshots.anatomical = null;
rebuildGraph();
if (state.graph !== null) throw Error('synthetic graph retained as anatomy');
`, context);
const synthetic = JSON.parse(fs.readFileSync(path.join(root, 'qa/fixtures/morphology/anatomical-stability.json')));
synthetic.mode = 'SyntheticColumns';
synthetic.paths = [];
synthetic.nodes.forEach((n, index) => { n.position_mm = {x:index-1.5, y:index%2, z:0}; });
const byId = new Map(synthetic.nodes.map(n => [n.id.value, n.position_mm]));
synthetic.edges.forEach(e => { e.points_mm = [byId.get(e.source.value), byId.get(e.target.value)]; });
context.state.snapshot.display_snapshots.synthetic_columns = synthetic;
context.state.render.layout = 'conventional';
vm.runInContext('rebuildGraph()', context);
assert.equal(context.state.graph.edges.length, synthetic.edges.length);
assert.equal(context.state.graph.membrane, null);
assert.ok(context.state.graph.edges.some(e => e.from.id === '4:1' && e.to.id === '2:1'), 'feedback edge retained');
console.log('PASS: bounded snapshots, path/soma projection, mode switching and synthetic connectivity');
if (process.argv.includes('--browser')) {
  const output = process.env.ANATOMY_QA_DIR || path.join(root, 'target/qa/anatomy-browser');
  fs.mkdirSync(output, { recursive: true });
  const file = path.join(output, 'canvas-fixture.html');
  // Keep the visual fixture pristine after the VM's deliberate mutations.
  const pristine = fs.readFileSync(path.join(root, 'qa/fixtures/morphology/anatomical-stability.json'), 'utf8');
  fs.writeFileSync(file, `<!doctype html><meta charset="utf-8"><style>body{margin:0;background:#181818;color:white}canvas{width:800px;height:600px}</style><canvas width="800" height="600"></canvas><pre id="result"></pre><script>
  const canvas = document.querySelector('canvas'), ctx = canvas.getContext('2d'), supportsCanvas2d = true;
  const edgeCountEl = document.createElement('span');
  function renderInstrumentation() {}
  const state = {snapshot:{display_snapshots:{anatomical:${pristine}}},render:{layout:'aarnn',showRegionLabels:false},
    view:{offsetX:0,offsetY:0,zoom:1,rotation:0},placement:{selectedLayers:new Set()},instrumentation:{},activity:{}};
  ${functions}
  function buildGraph(snapshot, layout) { return buildContractGraph(snapshot, layout); }
  try {
    rebuildGraph();
    const reference = canvas.toDataURL();
    for(let i=0;i<20;i++) {
      state.snapshot.display_snapshots.anatomical.sequence++;
      state.snapshot.display_snapshots.anatomical.coverage.complete = i%2===0;
      state.snapshot.display_snapshots.anatomical.coverage.truncated = i%2!==0;
      rebuildGraph();
      if(canvas.toDataURL() !== reference) throw Error('pixels changed across unchanged snapshots');
    }
    // Inject the reported long-pipe failure and verify every painted pixel
    // remains inside the membrane, including tube radius and marker discs.
    state.graph.edges.push({from:state.graph.nodes.sensory[0],to:state.graph.nodes.sensory[0],kind:'axon',radius:0.03,points:[{x:-20,y:-8},{x:0,y:0},{x:20,y:6}]});
    drawNetwork();
    const rgba=ctx.getImageData(0,0,800,600).data;
    let painted=0;
    for(let y=0;y<600;y++) for(let x=0;x<800;x++) if(rgba[(y*800+x)*4+3]>0) {
      painted++;
      const q=((x+.5-400)/192)**2+((y+.5-300)/134.4)**2;
      if(q>1.03) throw Error('pixel outside membrane '+x+','+y);
    }
    if(painted<10000) throw Error('empty render');
    rebuildGraph();
    document.getElementById('result').textContent='PASS: repeated canvas pixels and membrane clipping';
  } catch(e) {document.getElementById('result').textContent='FAIL: '+e.stack;}
  </script>`);
  const chrome = process.env.NM_CHROMIUM || 'google-chrome';
  const args = ['--headless', '--no-sandbox', '--disable-dev-shm-usage', '--disable-background-networking', '--no-first-run', '--no-default-browser-check',
    `--user-data-dir=${path.join(output, 'chrome-profile')}`, '--window-size=800,660', `--screenshot=${path.join(output, 'web-canvas.png')}`, '--dump-dom', `file://${file}`];
  const run = spawnSync(chrome, args, { encoding: 'utf8', timeout: 30000, maxBuffer: 4*1024*1024 });
  fs.writeFileSync(path.join(output, 'chrome.log'), run.stderr || String(run.error || ''));
  assert.equal(run.status, 0, run.error || run.stderr);
  const status = run.stdout.match(/<pre id="result">([\s\S]*?)<\/pre>/)?.[1] || '';
  fs.writeFileSync(path.join(output, 'browser-result.txt'), status);
  assert.ok(status.startsWith('PASS:'), status);
  console.log(status);
}

if (process.argv.includes('--sustained')) {
  const output = process.env.ANATOMY_QA_DIR;
  assert.ok(output, 'sustained lane needs captured growth');
  const captures = [];
  for (let step=0; step<=1200; step+=100) {
    const suffix = String(step).padStart(4, '0');
    const anatomical = JSON.parse(fs.readFileSync(path.join(output, `frame-${suffix}.json`)));
    const synthetic = JSON.parse(fs.readFileSync(path.join(output, `synthetic-${suffix}.json`)));
    for (const [layout, key, snapshot] of [['aarnn','anatomical',anatomical], ['conventional','synthetic_columns',synthetic]]) {
      context.state.snapshot = {display_snapshots:{[key]:snapshot}};
      context.state.render.layout = layout;
      context.state.graph = null;
      vm.runInContext('rebuildGraph()', context);
      const graph = context.state.graph;
      const nodes = [...graph.nodes.sensory, ...graph.nodes.hidden.flat(), ...graph.nodes.output];
      assert.equal(new Set(nodes.map(n=>n.id)).size, snapshot.nodes.length, `rounded/duplicate identities at ${step}`);
      assert.equal(graph.edges.length, snapshot.edges.length + snapshot.paths.length, `missing geometry at ${step}`);
      snapshot.paths.forEach((p,index) => assert.equal(graph.edges[snapshot.edges.length+index].from.id, `${p.owner.value}:${p.owner.generation}`));
    }
    if ([400,500,1200].includes(step)) captures.push({step,anatomical,synthetic});
  }
  fs.writeFileSync(path.join(output, 'growth-data.js'), `const captures=${JSON.stringify(captures)};`);
  const file = path.join(output, 'growth-canvas.html');
  fs.writeFileSync(file, `<!doctype html><meta charset="utf-8"><style>body{margin:0;background:#181818;color:white;font:14px sans-serif}#frames{display:grid;grid-template-columns:repeat(3,400px)}canvas{width:400px;height:300px}p{margin:4px}</style><div id="frames"></div><pre id="result"></pre><script src="growth-data.js"></script><script>
  let canvas, ctx;
  const supportsCanvas2d=true, edgeCountEl=document.createElement('span');
  function renderInstrumentation() {}
  const state={snapshot:{},render:{layout:'aarnn',showRegionLabels:false},view:{offsetX:0,offsetY:0,zoom:1,rotation:0},placement:{selectedLayers:new Set()},instrumentation:{},activity:{}};
  ${functions}
  function buildGraph(snapshot,layout){return buildContractGraph(snapshot,layout);}
  try {
    for(const layout of ['aarnn','conventional']) for(const capture of captures) {
      const box=document.createElement('div'),label=document.createElement('p');
      label.textContent=capture.step+' ms: '+(layout==='aarnn'?'anatomical':'synthetic columns');box.append(label);
      canvas=document.createElement('canvas');canvas.width=800;canvas.height=600;box.append(canvas);document.getElementById('frames').append(box);ctx=canvas.getContext('2d');
      state.snapshot={display_snapshots:{anatomical:capture.anatomical,synthetic_columns:capture.synthetic}};state.render.layout=layout;state.graph=null;rebuildGraph();
      const pixels=canvas.toDataURL();drawNetwork();if(canvas.toDataURL()!==pixels)throw Error('unstable captured frame');
    }
    document.getElementById('result').textContent='PASS: 13 growth states, exact identities/owners, both modes, repeated pixels';
  }catch(e){document.getElementById('result').textContent='FAIL: '+e.stack;}
  </script>`);
  const run=spawnSync(process.env.NM_CHROMIUM || 'google-chrome', ['--headless','--no-sandbox','--disable-dev-shm-usage','--disable-background-networking','--no-first-run','--no-default-browser-check',`--user-data-dir=${path.join(output,'growth-chrome-profile')}`,'--window-size=1200,730',`--screenshot=${path.join(output,'web-growth.png')}`,'--dump-dom',`file://${file}`], {encoding:'utf8',timeout:45000,maxBuffer:4*1024*1024});
  fs.writeFileSync(path.join(output,'growth-chrome.log'), run.stderr || String(run.error || ''));
  assert.equal(run.status,0,run.error || run.stderr);
  const status=run.stdout.match(/<pre id="result">([\s\S]*?)<\/pre>/)?.[1] || '';
  fs.writeFileSync(path.join(output,'growth-browser-result.txt'),status);
  assert.ok(status.startsWith('PASS:'),status);
  console.log(status);
}
