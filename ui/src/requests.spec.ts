// @vitest-environment jsdom
import { describe, expect, test } from 'vitest';
import type { DumpsStatus, RequestDetail, RequestSummary } from './api';
import { createRequestsStore, rows, timeline, type RequestsApi } from './requests.svelte';

const summary = (id: number, site: string | null, findings: string[] = []): RequestSummary => ({
  id, time: 1_800_000_000_000 + id, site, sapi: 'fpm-fcgi', method: 'GET', uri: `/r${id}`, command: null, status: 200,
  ms: 40, memory: 1, queryCount: 2, queryMs: 3, dropped: 0, findings,
});

const status: DumpsStatus = { port: 2306, listening: true, error: null, enabled: true, requests: true };

function fakeApi(over: Partial<RequestsApi> = {}) {
  let all = [summary(3, 'shop.test', ['n+1']), summary(2, null), summary(1, 'shop.test')];
  const calls: string[] = [];
  const api: RequestsApi & { calls: string[]; push(s: RequestSummary): void } = {
    calls,
    push(s) {
      all = [s, ...all];
    },
    dumpsStatus: async () => ({ ...status }),
    requestsList: async (after) => {
      calls.push(`list ${after ?? 'all'}`);
      return structuredClone(all.filter((r) => after === undefined || r.id > after));
    },
    requestsGet: async (id) => {
      calls.push(`get ${id}`);
      const s = all.find((r) => r.id === id)!;
      return { ...s, queries: [], findings: [] } satisfies RequestDetail;
    },
    requestsClear: async () => {
      calls.push('clear');
      all = [];
    },
    requestsEnable: async (on) => ({ ...status, requests: on }),
    ...over,
  };
  return api;
}

describe('Requests store', () => {
  test('sites filter the list and Clear empties it', async () => {
    const api = fakeApi();
    const store = createRequestsStore(api);
    await store.load();
    expect(store.sites.map((s) => [s.label, s.count])).toEqual([['shop.test', 2], ['Unknown', 1]]);
    store.filter = 'shop.test';
    expect(store.requests.map((r) => r.id)).toEqual([3, 1]);
    await store.open(3);
    await store.clear();
    expect(store.requests).toEqual([]);
    expect(store.current).toBe(null);
  });

  test('the poll asks only for newer requests', async () => {
    const api = fakeApi();
    const store = createRequestsStore(api);
    await store.load();
    api.push(summary(4, null));
    await store.refresh();
    await store.refresh();
    expect(api.calls).toEqual(['list all', 'list 3', 'list 4']);
    expect(store.requests.map((r) => r.id)).toEqual([4, 3, 2, 1]);
  });

  test('opening loads the whole request; the switch calls requests.enable', async () => {
    const api = fakeApi();
    const store = createRequestsStore(api);
    await store.load();
    await store.open(2);
    expect(api.calls).toContain('get 2');
    expect(store.current?.id).toBe(2);
    await store.setEnabled(false);
    expect(store.status?.requests).toBe(false);
  });
});

describe('rows', () => {
  test('an N+1 group shows its first query with the count of the rest, until it is opened', () => {
    const findings = [
      { kind: 'n+1' as const, text: '', queries: [1, 2, 3, 5] },
      { kind: 'duplicate' as const, text: '', queries: [0, 4] },
    ];
    expect(rows(6, findings, new Set())).toEqual([
      { index: 0, more: 0 },
      { index: 1, more: 3 },
      { index: 4, more: 0 },
    ]);
    expect(rows(6, findings, new Set([1]))).toEqual([0, 1, 2, 3, 4, 5].map((index) => ({ index, more: 0 })));
  });
});

describe('timeline', () => {
  test('start and duration become percentages of the request, never thinner than a hairline', () => {
    const q = (start: number, ms: number) => ({ sql: '', bindings: [], ms, startMs: start, connection: '', file: null, line: null });
    expect(timeline([q(10, 20), q(50, 0)], 100)).toEqual([
      { left: 10, width: 20 },
      { left: 50, width: 0.4 },
    ]);
    // A query reported past the request's end stretches the scale instead of overflowing.
    expect(timeline([q(150, 50)], 100)).toEqual([{ left: 75, width: 25 }]);
  });
});

describe('requests store loading', () => {
  test('is loading until the first answer, also when it fails', async () => {
    const store = createRequestsStore(fakeApi());
    expect(store.loaded).toBe(false);
    await store.load();
    expect(store.loaded).toBe(true);
    const failing = createRequestsStore(fakeApi({ dumpsStatus: async () => { throw 'down'; } }));
    await failing.load().catch(() => {});
    expect(failing.loaded).toBe(true);
  });
});
