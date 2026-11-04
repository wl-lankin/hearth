// @vitest-environment jsdom
import { flushSync, mount, unmount } from 'svelte';
import { expect, test, vi } from 'vitest';
import * as api from './api';
import type { MoveCheck } from './api';
import { createMoveStore } from './moveover.svelte';
import Home from './views/Home.svelte';
import MoveOver from './views/MoveOver.svelte';

async function settle(rounds = 8) {
  for (let i = 0; i < rounds; i++) {
    await new Promise((r) => setTimeout(r, 15));
    flushSync();
  }
}

const red: MoveCheck = {
  sites: [
    { host: 'shop.test', scheme: 'http', verdict: { kind: 'same' } },
    { host: 'blog.test', scheme: 'http', verdict: { kind: 'changed', detail: '302 to / instead of 200' } },
  ],
  services: [{ name: 'Redis 8', running: true }],
  node: { fromHearth: true },
  accepted: [],
};

function fakeApi(state = { herd: true, moved: false, uninstalled: false }) {
  return {
    ...api,
    moveoverState: vi.fn(async () => state),
    moveoverApply: vi.fn(async () => ({ jobId: 1 })),
    moveoverCheck: vi.fn(async () => structuredClone(red)),
    moveoverAccept: vi.fn(async (key: string) => ({ accepted: [key] })),
    moveoverUndo: vi.fn(async () => ({ jobId: 2 })),
    moveoverUninstall: vi.fn(async () => ({ exitCode: 0, check: { ...structuredClone(red), accepted: ['http://blog.test'] } })),
    jobGet: vi.fn(async (id: number) => ({
      id, kind: 'moveover', key: '', label: '', progress: 1, message: 'Moved', state: 'done' as const, error: null,
    })),
  };
}

function render(source: ReturnType<typeof fakeApi>) {
  const target = document.createElement('div');
  document.body.append(target);
  const store = createMoveStore(source, { pollMs: 5 });
  const page = mount(MoveOver, { target, props: { store } });
  const button = (label: string) =>
    [...target.querySelectorAll('button')].find((b) => b.textContent?.trim() === label) as HTMLButtonElement | undefined;
  return { target, page, store, button, text: () => target.textContent?.replace(/\s+/g, ' ') ?? '' };
}

test('the dry run lists what moves and, muted with its reason, what stays', async () => {
  const { page, target, text, button } = render(fakeApi());
  await settle();
  expect(text()).toContain('shop.test');
  expect(text()).toContain("Hearth's PHP builds have no mongodb extension");
  const stays = [...target.querySelectorAll('.item.stays')].map((r) => r.textContent ?? '');
  expect(stays.some((t) => t.includes('MariaDB 10.11'))).toBe(true);
  expect(button('Move over')).toBeTruthy();
  unmount(page);
});

test('moving leads to the check, and the uninstall waits for every red row', async () => {
  const source = fakeApi();
  const { page, text, button } = render(source);
  await settle();
  button('Move over')!.click();
  await settle(12);
  expect(source.moveoverApply).toHaveBeenCalled();
  expect(text()).toContain('302 to / instead of 200');
  expect(button('Uninstall Herd')!.disabled).toBe(true);
  button('Accept')!.click();
  await settle();
  expect(source.moveoverAccept).toHaveBeenCalledWith('http://blog.test');
  expect(button('Uninstall Herd')!.disabled).toBe(false);
  expect(button('Undo')).toBeTruthy();
  button('Uninstall Herd')!.click();
  await settle();
  expect(source.moveoverUninstall).toHaveBeenCalled();
  expect(text()).toContain('Claude updates your CLAUDE.md and skills');
  expect(button('Undo')).toBeUndefined();
  unmount(page);
});

test('Home offers the move only while Herd is there and nothing moved yet', async () => {
  const target = document.createElement('div');
  document.body.append(target);
  const show = mount(Home, { target, props: { onnavigate: vi.fn(), source: fakeApi() } });
  await settle();
  expect(target.textContent).toContain('Move over from Herd');
  unmount(show);
  const moved = mount(Home, { target, props: { onnavigate: vi.fn(), source: fakeApi({ herd: true, moved: true, uninstalled: false }) } });
  await settle();
  expect(target.textContent).not.toContain('Move over from Herd');
  unmount(moved);
});
