/**
 * An app, not a web page: right-click opens no browser menu (Reload, Inspect, Save as...).
 * Copying and pasting with the keyboard still works in fields.
 */
export function blockContextMenu(root: Document): void {
  root.addEventListener('contextmenu', (event) => event.preventDefault());
}
