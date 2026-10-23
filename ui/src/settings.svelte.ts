import type { Settings } from './api';

type SettingsApi = {
  getSettings(): Promise<Settings>;
  setSettings(settings: Settings): Promise<Settings>;
};

const defaults: Settings = { theme: 'system', launchAtLogin: false, startHidden: false };

/** The settings as the UI shows them: changes apply at once and roll back when the app refuses them. */
export function createSettingsStore(api: SettingsApi) {
  let value = $state<Settings>({ ...defaults });
  let error = $state<string | null>(null);

  return {
    get value() {
      return value;
    },
    get error() {
      return error;
    },
    async load() {
      value = await api.getSettings();
    },
    async update(patch: Partial<Settings>) {
      const before = value;
      const next = { ...value, ...patch };
      value = next;
      try {
        value = await api.setSettings(next);
        error = null;
      } catch (e) {
        value = before;
        error = typeof e === 'string' ? e : e instanceof Error ? e.message : String(e);
      }
    },
  };
}

export type SettingsStore = ReturnType<typeof createSettingsStore>;
