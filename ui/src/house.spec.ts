import { expect, test } from 'vitest';
import { greeting, litWindows, rack, windows } from './house';

const all = ['terminal', 'dormer', 'left', 'door'];

test('the web server lights the terminal window, each running service one more window', () => {
  expect(litWindows(false, 0)).toEqual({ windows: [], rack: false });
  expect(litWindows(true, 0)).toEqual({ windows: ['terminal'], rack: false });
  expect(litWindows(true, 2)).toEqual({ windows: ['terminal', 'dormer', 'left'], rack: false });
  expect(litWindows(true, 3)).toEqual({ windows: all, rack: false });
  expect(litWindows(false, 1)).toEqual({ windows: ['dormer'], rack: false });
});

test('more services than windows light the rack; nonsense counts light nothing extra', () => {
  expect(litWindows(true, 9)).toEqual({ windows: all, rack: true });
  expect(litWindows(true, -1)).toEqual(litWindows(true, 0));
});

test('every window has a place on the picture', () => {
  expect(windows.map((w) => w.id)).toEqual(all);
  for (const w of [...windows, ...rack]) {
    expect(w.x).toBeGreaterThanOrEqual(0);
    expect(w.y).toBeGreaterThanOrEqual(0);
    expect(w.x + w.w).toBeLessThanOrEqual(100);
    expect(w.y + w.h).toBeLessThanOrEqual(100);
  }
});

test('the greeting follows the time of day', () => {
  for (const h of [5, 11]) expect(greeting(h)).toBe('Good morning');
  for (const h of [12, 17]) expect(greeting(h)).toBe('Good afternoon');
  for (const h of [18, 22]) expect(greeting(h)).toBe('Good evening');
  for (const h of [23, 0, 4]) expect(greeting(h)).toBe('Good night');
});
