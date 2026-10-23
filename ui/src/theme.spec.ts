import { expect, test } from 'vitest';
import { resolveTheme } from './theme';

test('system follows the OS preference', () => {
  expect(resolveTheme('system', true)).toBe('dark');
  expect(resolveTheme('system', false)).toBe('light');
});

test('an explicit choice wins over the OS preference', () => {
  expect(resolveTheme('light', true)).toBe('light');
  expect(resolveTheme('dark', false)).toBe('dark');
});
