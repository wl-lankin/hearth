// Hearth's version lives in three files that must agree: package.json, src-tauri/tauri.conf.json and
// the workspace Cargo.toml. `npm run version:check` and the release workflow use these helpers.

/** The `version` under `[workspace.package]` in a Cargo.toml, or null. */
export function cargoWorkspaceVersion(toml) {
  const section = toml.split(/^\[/m).find((part) => part.startsWith('workspace.package]'));
  const match = section?.match(/^version\s*=\s*"([^"]+)"/m);
  return match ? match[1] : null;
}

export function versionsMatch({ pkg, tauri, cargo }) {
  if (pkg === tauri && tauri === cargo) return { ok: true, message: `All versions are ${pkg}` };
  return {
    ok: false,
    message: `Versions differ: package.json ${pkg}, tauri.conf.json ${tauri}, Cargo.toml ${cargo}`,
  };
}
