import { expect, test } from 'vitest';
import { describeUpdate } from './updates';

test('every update state has a plain status line', () => {
  expect(describeUpdate({ phase: 'idle' })).toBe('Not checked yet');
  expect(describeUpdate({ phase: 'checking' })).toBe('Checking for updates');
  expect(describeUpdate({ phase: 'current' })).toBe('Hearth is up to date');
  expect(describeUpdate({ phase: 'downloading', version: '0.2.0', progress: 0.45 })).toBe('Downloading 0.2.0 (45 %)');
  expect(describeUpdate({ phase: 'ready', version: '0.2.0' })).toBe('0.2.0 is ready');
  expect(describeUpdate({ phase: 'installing', version: '0.2.0' })).toBe('Installing 0.2.0');
  expect(describeUpdate({ phase: 'disabled' })).toBe('Updates are off in development builds');
  expect(describeUpdate({ phase: 'error', message: 'offline' })).toBe('Update check failed: offline');
});
