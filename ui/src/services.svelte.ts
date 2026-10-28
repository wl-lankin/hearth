import type { ServiceRow, ServicesAvailable } from './api';

export type ServicesApi = {
  servicesList(): Promise<ServiceRow[]>;
  servicesAvailable(): Promise<ServicesAvailable>;
  servicesSuggestPort(kind: string): Promise<number>;
  servicesCreate(kind: string, line: string, port: number, name?: string): Promise<void>;
  servicesStart(id: string): Promise<void>;
  servicesStop(id: string): Promise<void>;
  servicesDelete(id: string): Promise<void>;
  servicesAutostart(id: string, on: boolean): Promise<void>;
  servicesEnv(id: string): Promise<string>;
  servicesLog(id: string): Promise<string>;
};

export type Draft = { kind: string; line: string; port: number; name: string };

const asMessage = (e: unknown): string => (typeof e === 'string' ? e : e instanceof Error ? e.message : String(e));

export function createServicesStore(api: ServicesApi) {
  let services = $state<ServiceRow[]>([]);
  let available = $state<ServicesAvailable>({ builds: [], unavailable: [] });
  let draft = $state<Draft | null>(null);
  let createError = $state<string | null>(null);
  let confirming = $state<string | null>(null);
  let busy = $state<Record<string, boolean>>({});
  let errors = $state<Record<string, string | null>>({});
  let error = $state<string | null>(null);

  async function reload() {
    services = await api.servicesList();
  }

  /** Runs an action on one service; its failure shows on that row. */
  async function act(id: string, f: () => Promise<void>) {
    busy[id] = true;
    errors[id] = null;
    try {
      await f();
    } catch (e) {
      errors[id] = asMessage(e);
    } finally {
      busy[id] = false;
      await reload().catch(() => {});
    }
  }

  return {
    get services() {
      return services;
    },
    get unavailable() {
      return available.unavailable;
    },
    /** One entry per kind this computer can run, in the catalog's order. */
    get kinds() {
      const seen = new Map<string, string>();
      for (const b of available.builds) if (!seen.has(b.kind)) seen.set(b.kind, b.name);
      return [...seen].map(([kind, name]) => ({ kind, name }));
    },
    lines(kind: string) {
      return available.builds.filter((b) => b.kind === kind).map((b) => b.line);
    },
    get draft() {
      return draft;
    },
    get createError() {
      return createError;
    },
    /** The free port a "Port N is used ... (M is free)" error names. */
    get freePort(): number | null {
      const m = createError?.match(/\((\d+) is free\)/);
      return m ? Number(m[1]) : null;
    },
    get confirming() {
      return confirming;
    },
    get busy() {
      return busy;
    },
    get errors() {
      return errors;
    },
    get error() {
      return error;
    },
    async load() {
      try {
        [services, available] = await Promise.all([api.servicesList(), api.servicesAvailable()]);
        error = null;
      } catch (e) {
        error = asMessage(e);
      }
    },
    refresh: () => reload().catch(() => {}),
    /** Opens the form for a new service of `kind` with its first version and a free port. */
    async begin(kind: string) {
      createError = null;
      const line = available.builds.find((b) => b.kind === kind)?.line ?? '';
      draft = { kind, line, port: await api.servicesSuggestPort(kind), name: '' };
    },
    cancel() {
      draft = null;
      createError = null;
    },
    useFreePort() {
      const port = this.freePort;
      if (draft && port) draft.port = port;
      createError = null;
    },
    async create() {
      if (!draft) return;
      createError = null;
      try {
        await api.servicesCreate(draft.kind, draft.line, draft.port, draft.name || undefined);
        draft = null;
        await reload();
      } catch (e) {
        createError = asMessage(e);
      }
    },
    start: (id: string) => act(id, () => api.servicesStart(id)),
    stop: (id: string) => act(id, () => api.servicesStop(id)),
    askDelete(id: string) {
      confirming = id;
    },
    cancelDelete() {
      confirming = null;
    },
    async confirmDelete() {
      const id = confirming;
      confirming = null;
      if (id) await act(id, () => api.servicesDelete(id));
    },
    async setAutostart(id: string, on: boolean) {
      const row = services.find((s) => s.id === id);
      if (!row) return;
      const before = row.autostart;
      row.autostart = on;
      errors[id] = null;
      try {
        await api.servicesAutostart(id, on);
      } catch (e) {
        row.autostart = before;
        errors[id] = asMessage(e);
      }
    },
    /** Copies the service's .env lines; `write` is the clipboard (injected for tests). */
    async copyEnv(id: string, write: (text: string) => Promise<void> = (t) => navigator.clipboard.writeText(t)) {
      try {
        await write(await api.servicesEnv(id));
      } catch (e) {
        errors[id] = asMessage(e);
      }
    },
    log: (id: string) => api.servicesLog(id),
  };
}

export type ServicesStore = ReturnType<typeof createServicesStore>;
