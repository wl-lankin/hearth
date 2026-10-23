import { expect, test } from 'vitest';
import { formatUptime } from './format';

test('seconds, minutes, hours and days', () => {
  expect(formatUptime(0)).toBe('0s');
  expect(formatUptime(42)).toBe('42s');
  expect(formatUptime(125)).toBe('2m 5s');
  expect(formatUptime(7500)).toBe('2h 5m');
  expect(formatUptime(90000)).toBe('1d 1h');
});
