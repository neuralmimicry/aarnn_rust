const fs = require('node:fs');
const path = require('node:path');
const vm = require('node:vm');
const test = require('node:test');
const assert = require('node:assert/strict');

const root = path.resolve(__dirname, '../..');
const source = fs.readFileSync(path.join(root, 'web_ui/app.js'), 'utf8');
const configStart = source.indexOf('const workspaceDisplayProjectionRetryDelaysMs =');
const start = source.indexOf('function fetchWorkspaceDisplayProjection(');
const end = source.indexOf('\nfunction managementRequestId(', start);
assert.ok(configStart >= 0 && start > configStart && end > start, 'production workspace projection fetcher exists');
const fetcherSource = source.slice(configStart, end);

function makeResponse(status, payload) {
  return {
    status,
    ok: status >= 200 && status < 300,
    clone() { return { json: async () => payload }; },
    json: async () => payload
  };
}

function makeFetcher(runtimeFetch) {
  const delays = [];
  const context = {
    workspaceDisplayProjectionRequestTail: Promise.resolve(),
    runtimeFetch,
    Math: { random: () => 0, floor: Math.floor, min: Math.min, max: Math.max },
    setTimeout(callback, delayMs) {
      delays.push(delayMs);
      queueMicrotask(callback);
    }
  };
  vm.createContext(context);
  vm.runInContext(fetcherSource, context);
  return { fetch: context.fetchWorkspaceDisplayProjection, delays };
}

test('workspace projection retries a retryable capacity 503 with bounded backoff', async () => {
  let calls = 0;
  const retries = [];
  const client = makeFetcher(async () => {
    calls += 1;
    return calls < 3
      ? makeResponse(503, { error: 'workspace display projection capacity is busy', retryable: true })
      : makeResponse(200, { display_snapshots: { anatomical: { nodes: [{}, {}] } } });
  });

  const response = await client.fetch('/api/runtime/workspaces/system/snapshot', {
    onBusyRetry: retry => retries.push(retry)
  });

  assert.equal(response.status, 200);
  assert.equal(calls, 3);
  assert.deepEqual(client.delays, [500, 1000]);
  assert.deepEqual(retries.map(retry => retry.attempt), [1, 2]);
});

test('workspace projection does not retry non-retryable 503 responses', async () => {
  let calls = 0;
  const client = makeFetcher(async () => {
    calls += 1;
    return makeResponse(503, { error: 'workspace is unavailable', retryable: false });
  });

  const response = await client.fetch('/api/runtime/workspaces/system/snapshot');

  assert.equal(response.status, 503);
  assert.equal(calls, 1);
  assert.deepEqual(client.delays, []);
});

test('workspace projection stops after the configured retry budget', async () => {
  let calls = 0;
  const retries = [];
  const client = makeFetcher(async () => {
    calls += 1;
    return makeResponse(503, { error: 'busy', retryable: true });
  });

  const response = await client.fetch('/api/runtime/workspaces/system/snapshot', {
    onBusyRetry: retry => retries.push(retry)
  });

  assert.equal(response.status, 503);
  assert.equal(calls, 9);
  assert.equal(client.delays.length, 8);
  assert.equal(retries.length, 8);
  assert.ok(client.delays.reduce((sum, delay) => sum + delay, 0) >= 26_500);
});

test('workspace projection requests remain serialized within one browser page', async () => {
  let active = 0;
  let maximumActive = 0;
  const client = makeFetcher(async () => {
    active += 1;
    maximumActive = Math.max(maximumActive, active);
    await Promise.resolve();
    active -= 1;
    return makeResponse(200, {});
  });

  await Promise.all([
    client.fetch('/api/runtime/workspaces/first/snapshot'),
    client.fetch('/api/runtime/workspaces/second/snapshot')
  ]);

  assert.equal(maximumActive, 1);
});
