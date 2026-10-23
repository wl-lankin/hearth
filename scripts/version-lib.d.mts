export function cargoWorkspaceVersion(toml: string): string | null;
export function versionsMatch(versions: { pkg: string; tauri: string; cargo: string | null }): {
  ok: boolean;
  message: string;
};
