import type { ThemeSetting } from './api';

export type Theme = 'light' | 'dark';

export function resolveTheme(setting: ThemeSetting, prefersDark: boolean): Theme {
  if (setting === 'system') return prefersDark ? 'dark' : 'light';
  return setting;
}

export function applyTheme(theme: Theme): void {
  document.documentElement.dataset.theme = theme;
}
