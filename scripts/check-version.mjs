// Fails when package.json, tauri.conf.json and Cargo.toml disagree on the version.
// Usage: node scripts/check-version.mjs [expected-version]
import { readFileSync } from 'node:fs';
import { cargoWorkspaceVersion, versionsMatch } from './version-lib.mjs';

const read = (path) => readFileSync(new URL(`../${path}`, import.meta.url), 'utf8');
const versions = {
  pkg: JSON.parse(read('package.json')).version,
  tauri: JSON.parse(read('src-tauri/tauri.conf.json')).version,
  cargo: cargoWorkspaceVersion(read('Cargo.toml')),
};
const result = versionsMatch(versions);
const expected = process.argv[2];
if (result.ok && expected && versions.pkg !== expected) {
  console.error(`The versions are ${versions.pkg}, expected ${expected}`);
  process.exit(1);
}
console[result.ok ? 'log' : 'error'](result.message);
process.exit(result.ok ? 0 : 1);
