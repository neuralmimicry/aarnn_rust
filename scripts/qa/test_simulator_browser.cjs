/* Optional real-browser SIM-CONTENT-001 lane. Requires Playwright and Chromium.
 * Serves only local assets and mocks the neural gateway; no live brain is used. */
const { chromium } = require('playwright');
const fs = require('node:fs');
const path = require('node:path');
const http = require('node:http');
const assert = require('node:assert/strict');

(async () => {
  const root = path.resolve(__dirname, '../../web_ui');
  const output = process.env.NM_SIM_CONTENT_RESULT_DIR || path.resolve(__dirname, '../../target/qa/simulator-content/browser-' + Date.now());
  fs.mkdirSync(output, { recursive: true });
  const server = http.createServer((request, response) => {
    const file = path.resolve(root, '.' + new URL(request.url, 'http://localhost').pathname);
    if (!file.startsWith(root + path.sep) || !fs.existsSync(file) || !fs.statSync(file).isFile()) {
      response.writeHead(404).end(); return;
    }
    response.setHeader('Content-Type', file.endsWith('.js') ? 'application/javascript' : file.endsWith('.css') ? 'text/css' : 'text/html');
    response.end(fs.readFileSync(file));
  });
  await new Promise(resolve => server.listen(0, '127.0.0.1', resolve));
  let browser;
  try {
    browser = await chromium.launch({ executablePath: process.env.NM_CHROMIUM || undefined, headless: true,
      args: ['--use-angle=swiftshader', '--enable-unsafe-swiftshader'] });
    const page = await browser.newPage({ viewport: { width: 1440, height: 1100 } });
    const errors = [], requests = [];
    page.on('pageerror', e => errors.push(e.message));
    await page.route('**/api/config', r => r.fulfill({ json: { default_network: 'celegans_0' } }));
    await page.route('**/api/aer/infer', r => {
      const q = r.request().postDataJSON(); requests.push(q);
      return r.fulfill({ json: { output_spike_indices: [0, 1, 2], output_step_index: q.step_index } });
    });
    await page.goto(`http://127.0.0.1:${server.address().port}/webgl-sim.html`);
    await page.waitForFunction(() => document.querySelector('#webgl-capability').textContent === 'WebGL available');
    for (const kind of ['celegans', 'drosophila_banc', 'drosophila_fafb', 'hexapod', 'nao', 'zebrafish']) {
      await page.selectOption('#webgl-robot', kind);
      await page.evaluate(() => new Promise(resolve => requestAnimationFrame(() => requestAnimationFrame(resolve))));
      await page.screenshot({ path: path.join(output, kind + '.png') });
    }
    await page.selectOption('#webgl-robot', 'celegans');
    await page.check('#webgl-anatomy');
    await page.screenshot({ path: path.join(output, 'celegans-anatomy.png') });
    await page.click('#webgl-connect');
    await page.waitForFunction(() => Number(document.querySelector('#webgl-step').textContent) > 2);
    await page.click('#webgl-connect');
    assert.equal(await page.locator('#webgl-transport').textContent(), 'disconnected');
    assert.ok(requests.every(q => q.input_values.length === 24 && q.dt_ms === 1));
    assert.ok(requests.every((q, i) => i === 0 || q.step_index === requests[i - 1].step_index + 1));
    await page.unroute('**/api/aer/infer');
    let pending, resolvePending;
    const intercepted = new Promise(resolve => { resolvePending = resolve; });
    await page.route('**/api/aer/infer', r => { pending = r; resolvePending(); });
    await page.click('#webgl-connect');
    await intercepted;
    await page.selectOption('#webgl-robot', 'hexapod');
    await pending.fulfill({ json: { output_spike_indices: [0], output_step_index: 999999 } }).catch(() => {});
    assert.equal(await page.locator('#webgl-transport').textContent(), 'disconnected');
    assert.notEqual(await page.locator('#webgl-step').textContent(), '999999');
    // A nonresponding gateway must release the single outstanding request.
    await page.click('#webgl-connect');
    await page.waitForFunction(() => document.querySelector('#webgl-transport').textContent === 'error', null, { timeout: 5000 });
    assert.equal(await page.locator('#webgl-connect').textContent(), 'Connect brain');
    await page.setViewportSize({ width: 390, height: 844 });
    await page.screenshot({ path: path.join(output, 'mobile.png') });
    assert.equal(await page.evaluate(() => document.documentElement.scrollWidth <= window.innerWidth), true);
    const report = { engine: await browser.version(), digest: await page.evaluate(() => window.NmSimContent.digest), profiles: 6, errors, requests: requests.length,
      scope: 'rendered content, anatomy, mocked inference, stale-response cancellation, timeout and mobile layout; no neural or biological validation' };
    fs.writeFileSync(path.join(output, 'browser.json'), JSON.stringify(report, null, 2));
    assert.deepEqual(errors, []);
    console.log(JSON.stringify(report));
  } finally {
    if (browser) await browser.close();
    await new Promise(resolve => server.close(resolve));
  }
})().catch(error => { console.error(error); process.exitCode = 1; });
