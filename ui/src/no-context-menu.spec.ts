// @vitest-environment jsdom
import { expect, test } from 'vitest';
import { blockContextMenu } from './no-context-menu';

test('right-click opens no menu anywhere, the keyboard still copies and pastes', () => {
  blockContextMenu(document);
  const field = document.createElement('input');
  document.body.append(field);
  for (const target of [document.body, field]) {
    const event = new MouseEvent('contextmenu', { bubbles: true, cancelable: true });
    target.dispatchEvent(event);
    expect(event.defaultPrevented).toBe(true);
  }
  const copy = new KeyboardEvent('keydown', { key: 'c', ctrlKey: true, bubbles: true, cancelable: true });
  field.dispatchEvent(copy);
  expect(copy.defaultPrevented).toBe(false);
});
