/** Hearth's illustrations, made by `scripts/make-illustrations.mjs`. */
import homeDark from './assets/illustrations/home-dark.webp?url';
import homeLit from './assets/illustrations/home-lit.webp?url';
import inspect from './assets/illustrations/inspect.webp?url';
import logs from './assets/illustrations/logs.webp?url';
import mail from './assets/illustrations/mail.webp?url';
import processes from './assets/illustrations/processes.webp?url';
import services from './assets/illustrations/services.webp?url';
import sites from './assets/illustrations/sites.webp?url';
import welcome from './assets/illustrations/welcome.webp?url';
import workspaces from './assets/illustrations/workspaces.webp?url';

export type Art = 'homeLit' | 'homeDark' | 'sites' | 'workspaces' | 'mail' | 'services' | 'processes' | 'logs' | 'inspect' | 'welcome';

/** The file name (without `.webp`) of each illustration. */
export const files: Record<Art, string> = {
  homeLit: 'home-lit',
  homeDark: 'home-dark',
  sites: 'sites',
  workspaces: 'workspaces',
  mail: 'mail',
  services: 'services',
  processes: 'processes',
  logs: 'logs',
  inspect: 'inspect',
  welcome: 'welcome',
};

/** Their URLs in the built app. */
export const art: Record<Art, string> = {
  homeLit,
  homeDark,
  sites,
  workspaces,
  mail,
  services,
  processes,
  logs,
  inspect,
  welcome,
};
