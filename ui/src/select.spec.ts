// @vitest-environment jsdom
import { flushSync, mount, unmount } from 'svelte';
import { expect, test } from 'vitest';
import Select from './Select.svelte';

const options = [
  { value: '', label: 'Default (8.4)' },
  { value: '8.4', label: 'PHP 8.4' },
  { value: '8.3', label: 'PHP 8.3' },
];

function setup(value = '') {
  const picked: string[] = [];
  const target = document.createElement('div');
  document.body.append(target);
  const select = mount(Select, { target, props: { value, options, label: 'PHP for shop', onchange: (v: string) => picked.push(v) } });
  const button = target.querySelector('button')!;
  return { target, button, picked, done: () => unmount(select) };
}

const key = (el: Element, k: string) => el.dispatchEvent(new KeyboardEvent('keydown', { key: k, bubbles: true }));

test('the button shows the chosen label; a click opens the list and picking an option reports it', () => {
  const { target, button, picked, done } = setup('8.3');
  expect(button.textContent).toContain('PHP 8.3');
  expect(button.getAttribute('aria-label')).toBe('PHP for shop');
  button.click();
  flushSync();
  const items = [...target.ownerDocument.querySelectorAll('[role="option"]')];
  expect(items.map((i) => i.textContent?.trim())).toEqual(['Default (8.4)', 'PHP 8.4', 'PHP 8.3']);
  expect(items[2].getAttribute('aria-selected')).toBe('true');
  (items[1] as HTMLElement).click();
  flushSync();
  expect(picked).toEqual(['8.4']);
  expect(target.ownerDocument.querySelector('[role="listbox"]')).toBeNull();
  done();
});

test('the keyboard opens, moves, picks and closes', () => {
  const { button, picked, done } = setup('');
  key(button, 'ArrowDown');
  flushSync();
  expect(document.querySelector('[role="listbox"]')).not.toBeNull();
  key(button, 'ArrowDown');
  key(button, 'ArrowDown');
  flushSync();
  key(button, 'Enter');
  flushSync();
  expect(picked).toEqual(['8.3']);
  key(button, 'ArrowDown');
  flushSync();
  key(button, 'Escape');
  flushSync();
  expect(document.querySelector('[role="listbox"]')).toBeNull();
  expect(picked).toEqual(['8.3']);
  done();
});

test('a click outside closes the list without picking', () => {
  const { button, picked, done } = setup('');
  button.click();
  flushSync();
  document.body.dispatchEvent(new MouseEvent('pointerdown', { bubbles: true }));
  flushSync();
  expect(document.querySelector('[role="listbox"]')).toBeNull();
  expect(picked).toEqual([]);
  done();
});
