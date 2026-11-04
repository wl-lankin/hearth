import type { ProcessList, ProcessOutput, ProcessRow, ProcessSuggestion, SitesList } from './api';

export type ProcessesApi = {
  processesList(): Promise<ProcessList>;
  processesOutput(site: string, name: string, offset?: number): Promise<ProcessOutput>;
  processesStart(site: string, name: string): Promise<void>;
  processesStop(site: string, name: string): Promise<void>;
  processesRestart(site: string, name: string): Promise<void>;
  processesAdd(site: string, name: string, command: string, autostart: boolean): Promise<void>;
  processesRemove(site: string, name: string): Promise<void>;
  processesSuggest(site: string): Promise<ProcessSuggestion[]>;
  sitesList(): Promise<SitesList>;
};

/** Output lines kept in memory; older ones drop off the top. */
const MAX_LINES = 2000;

const asMessage = (e: unknown): string => (typeof e === 'string' ? e : e instanceof Error ? e.message : String(e));

/** The last `MAX_LINES` lines of `text`. */
function capped(text: string): string {
  const lines = text.split('\n');
  // A trailing newline leaves an empty last element that is no line.
  const extra = lines.length - (text.endsWith('\n') ? 1 : 0) - MAX_LINES;
  return extra > 0 ? lines.slice(extra).join('\n') : text;
}

export function createProcessesStore(api: ProcessesApi) {
  let list = $state<ProcessList>({ sites: [] });
  let siteNames = $state<string[]>([]);
  let selected = $state<{ site: string; name: string } | null>(null);
  let output = $state('');
  let offset: number | undefined;
  let errors = $state<Record<string, string | null>>({});
  let error = $state<string | null>(null);
  let suggestions = $state<ProcessSuggestion[]>([]);
  let form = $state({ site: '', name: '', command: '', autostart: false });
  let formError = $state<string | null>(null);

  async function refresh() {
    try {
      list = await api.processesList();
      error = null;
    } catch (e) {
      error = asMessage(e);
    }
  }

  // One read at a time: two from the same offset would add the same text twice.
  let reading: Promise<void> | null = null;

  function readOutput(): Promise<void> {
    if (!reading) {
      reading = readOnce().finally(() => (reading = null));
    }
    return reading;
  }

  async function readOnce() {
    if (!selected) return;
    const { site, name } = selected;
    const from = offset;
    const got = await api.processesOutput(site, name, from);
    // The pick (or the read position) may have changed while this read ran.
    if (selected?.site !== site || selected?.name !== name || offset !== from) return;
    output = capped(got.reset || from === undefined ? got.text : output + got.text);
    offset = got.offset;
  }

  const rowKey = (site: string, name: string) => `${site}/${name}`;

  return {
    get sites() {
      return list.sites;
    },
    get siteNames() {
      return siteNames;
    },
    get selected() {
      return selected;
    },
    get output() {
      return output;
    },
    get error() {
      return error;
    },
    get suggestions() {
      return suggestions;
    },
    get form() {
      return form;
    },
    get formError() {
      return formError;
    },
    rowError: (site: string, name: string) => errors[rowKey(site, name)] ?? null,
    /** What a row shows: a crash waiting for its next try is still the supervisor's to stop. */
    state(p: ProcessRow): 'running' | 'restarting' | 'failed' | 'stopped' {
      if (p.running) return 'running';
      if (p.waiting) return 'restarting';
      return p.lastError ? 'failed' : 'stopped';
    },
    canStop: (p: ProcessRow) => p.running || p.waiting,
    async load() {
      await refresh();
      try {
        siteNames = (await api.sitesList()).sites.filter((s) => s.kind !== 'proxy').map((s) => s.name);
      } catch {
        siteNames = [];
      }
      if (!form.site) form.site = siteNames[0] ?? '';
    },
    /** Shows a process's output from its start (the last 256 KB). */
    async select(site: string, name: string) {
      selected = { site, name };
      output = '';
      offset = undefined;
      // A read for the previous pick may still run: this one starts after it.
      await reading;
      await readOutput().catch((e) => (error = asMessage(e)));
    },
    /** Called every second while the page is open: the states and the shown output's new text. */
    async poll() {
      await refresh();
      await readOutput().catch(() => {});
    },
    async act(kind: 'start' | 'stop' | 'restart', site: string, name: string) {
      errors[rowKey(site, name)] = null;
      try {
        await (kind === 'start' ? api.processesStart : kind === 'stop' ? api.processesStop : api.processesRestart)(site, name);
      } catch (e) {
        errors[rowKey(site, name)] = asMessage(e);
      }
      await refresh();
      if (selected?.site === site && selected.name === name) await readOutput().catch(() => {});
    },
    async suggest(site: string) {
      suggestions = await api.processesSuggest(site).catch(() => []);
    },
    useSuggestion(s: ProcessSuggestion) {
      form.name = s.name;
      form.command = s.command;
    },
    /** Adds the form's process; true when it was added (the form then empties). */
    async add(): Promise<boolean> {
      formError = null;
      try {
        await api.processesAdd(form.site, form.name.trim(), form.command, form.autostart);
      } catch (e) {
        formError = asMessage(e);
        return false;
      }
      form.name = '';
      form.command = '';
      form.autostart = false;
      await refresh();
      return true;
    },
    async remove(site: string, name: string) {
      try {
        await api.processesRemove(site, name);
      } catch (e) {
        errors[rowKey(site, name)] = asMessage(e);
        return;
      }
      if (selected?.site === site && selected.name === name) {
        selected = null;
        output = '';
        offset = undefined;
      }
      await refresh();
    },
  };
}
