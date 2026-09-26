const assert = require('node:assert/strict');
const fs = require('node:fs');
const vm = require('node:vm');

const window = {};
const source = fs.readFileSync('web_ui/service-access.js', 'utf8');
vm.runInNewContext(source, { window });

const indexHtml = fs.readFileSync('web_ui/index.html', 'utf8');
const bundleVersions = [...indexHtml.matchAll(/\/(?:service-access|management-client\.generated|aer-transport|app)\.js\?v=([^"']+)/g)]
  .map((match) => match[1]);
assert.equal(bundleVersions.length, 4, 'all browser bundles have cache-busted script URLs');
assert.equal(new Set(bundleVersions).size, 1, 'browser bundles use one published revision');

const admin = window.NMServiceAccess.getServiceAccess({
  authenticated: true,
  role: 'admin',
  groups: ['admin'],
  service_access: {
    aarnn: {
      access_level: 'control',
      public_access_level: 'request',
      visible_access_level: 'request',
      can_request: true,
      can_observe: false,
      can_use: true,
      can_control: true
    }
  }
}, 'aarnn');

assert.equal(admin.access_level, 'control');
assert.equal(admin.can_observe, true, 'control access includes observation even if a stored flag is stale');
assert.equal(admin.can_use, true);
assert.equal(admin.can_control, true);

const requestOnly = window.NMServiceAccess.getServiceAccess({
  authenticated: true,
  role: 'user',
  service_access: {
    aarnn: {
      access_level: 'request',
      public_access_level: 'request',
      visible_access_level: 'request',
      can_observe: false
    }
  }
}, 'aarnn');

assert.equal(requestOnly.can_observe, false, 'request access alone does not grant observation');

console.log('Browser service-access levels preserve inherited observation and request-only denial.');
