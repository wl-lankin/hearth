import { describe, expect, test } from 'vitest';
import { formatReceived, formatSize, formatUptime } from './format';

test('seconds, minutes, hours and days', () => {
  expect(formatUptime(0)).toBe('0s');
  expect(formatUptime(42)).toBe('42s');
  expect(formatUptime(125)).toBe('2m 5s');
  expect(formatUptime(7500)).toBe('2h 5m');
  expect(formatUptime(90000)).toBe('1d 1h');
});

describe('formatReceived', () => {
  const at = (iso: string) => Math.floor(new Date(iso).getTime() / 1000);
  test('today shows the time, this year the day, older ones the year too', () => {
    const now = new Date('2026-11-02T15:00:00');
    expect(formatReceived(at('2026-11-02T09:05:00'), now)).toBe('09:05');
    expect(formatReceived(at('2026-10-30T09:05:00'), now)).toBe('30 Oct');
    expect(formatReceived(at('2025-12-24T09:05:00'), now)).toBe('24 Dec 2025');
  });
});

describe('formatSize', () => {
  test('bytes, KB and MB', () => {
    expect(formatSize(900)).toBe('900 B');
    expect(formatSize(41230)).toBe('40 KB');
    expect(formatSize(3 * 1024 * 1024 + 1)).toBe('3.0 MB');
  });
});
