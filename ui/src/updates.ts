import type { UpdateState } from './api';

/** The one status line Settings shows for the updater. */
export function describeUpdate(state: UpdateState): string {
  switch (state.phase) {
    case 'idle':
      return 'Not checked yet';
    case 'disabled':
      return 'Updates are off in development builds';
    case 'checking':
      return 'Checking for updates';
    case 'current':
      return 'Hearth is up to date';
    case 'downloading':
      return `Downloading ${state.version} (${Math.round(state.progress * 100)} %)`;
    case 'ready':
      return `${state.version} is ready`;
    case 'installing':
      return `Installing ${state.version}`;
    case 'error':
      return `Update check failed: ${state.message}`;
  }
}
