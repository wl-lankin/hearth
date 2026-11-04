// Turns the illustrations (transparent 2k PNGs made with Higgsfield) into the small WebP files the
// UI ships. Run with `npm run illustrations -- <folder with the PNGs>` after changing one.
import { mkdirSync } from 'node:fs';
import { join } from 'node:path';
import sharp from 'sharp';

const source = process.argv[2];
if (!source) {
  console.error('Usage: npm run illustrations -- <folder with the PNGs>');
  process.exit(1);
}

const root = new URL('..', import.meta.url).pathname.replace(/^\/([A-Za-z]:)/, '$1');
const out = join(root, 'ui/src/assets/illustrations');
mkdirSync(out, { recursive: true });

// Source PNG, target name, width in pixels (about twice the size shown).
const items = [
  ['hero-2.png', 'home-lit', 1400],
  ['hero-2-dark.png', 'home-dark', 1400],
  ['empty-7.png', 'welcome', 1400],
  ['empty-0.png', 'sites', 720],
  ['empty-1b.png', 'workspaces', 720],
  ['empty-2b.png', 'mail', 720],
  ['empty-3.png', 'services', 720],
  ['empty-4.png', 'processes', 720],
  ['empty-5.png', 'logs', 720],
  ['empty-6b.png', 'inspect', 720],
];

for (const [file, name, width] of items) {
  const info = await sharp(join(source, file))
    .resize({ width })
    .webp({ quality: 82, alphaQuality: 90, effort: 6 })
    .toFile(join(out, `${name}.webp`));
  console.log(`${name}.webp ${Math.round(info.size / 1024)} KB`);
}
