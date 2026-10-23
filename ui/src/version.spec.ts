import { expect, test } from 'vitest';
import { cargoWorkspaceVersion, versionsMatch } from '../../scripts/version-lib.mjs';

test('equal versions match', () => {
  expect(versionsMatch({ pkg: '0.2.0', tauri: '0.2.0', cargo: '0.2.0' })).toEqual({ ok: true, message: 'All versions are 0.2.0' });
});

test('a mismatch names the odd file', () => {
  const result = versionsMatch({ pkg: '0.2.0', tauri: '0.2.0', cargo: '0.1.0' });
  expect(result.ok).toBe(false);
  expect(result.message).toBe('Versions differ: package.json 0.2.0, tauri.conf.json 0.2.0, Cargo.toml 0.1.0');
});

test('the workspace version is read from [workspace.package]', () => {
  const toml = '[workspace]\nmembers = ["a"]\n\n[workspace.package]\nversion = "1.4.2"\nedition = "2021"\n\n[profile.release]\n';
  expect(cargoWorkspaceVersion(toml)).toBe('1.4.2');
});
