// Renders the brand SVGs and lets the Tauri CLI build every app icon size from them.
// Run with `npm run icons` after changing assets/logo.svg or assets/tray-template.svg.
import { execFileSync } from 'node:child_process';
import { mkdirSync, mkdtempSync, readFileSync, rmSync, writeFileSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { join } from 'node:path';
import { Resvg } from '@resvg/resvg-js';

const root = new URL('..', import.meta.url).pathname.replace(/^\/([A-Za-z]:)/, '$1');
const render = (svg, size) =>
  new Resvg(readFileSync(join(root, svg)), { fitTo: { mode: 'width', value: size } }).render().asPng();

const tauri = (args) => execFileSync('npx', ['tauri', ...args], { cwd: root, stdio: 'inherit', shell: true });

const work = mkdtempSync(join(tmpdir(), 'hearth-icons-'));
try {
  const logo = join(work, 'logo.png');
  writeFileSync(logo, render('assets/logo.svg', 1024));
  tauri(['icon', logo, '-o', 'src-tauri/icons']);
  // Hearth ships for Windows and macOS only.
  for (const dir of ['android', 'ios']) rmSync(join(root, 'src-tauri/icons', dir), { recursive: true, force: true });

  // Tray: the color logo on Windows, a one-color template image macOS tints for the menu bar.
  writeFileSync(join(root, 'src-tauri/icons/tray.png'), render('assets/logo.svg', 64));
  writeFileSync(join(root, 'src-tauri/icons/tray-template.png'), render('assets/tray-template.svg', 44));

  // Brand image for the README.
  mkdirSync(join(root, 'docs'), { recursive: true });
  writeFileSync(join(root, 'docs/logo.png'), render('assets/logo.svg', 512));
} finally {
  rmSync(work, { recursive: true, force: true });
}
