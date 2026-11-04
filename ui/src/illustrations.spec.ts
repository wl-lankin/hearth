/// <reference types="node" />
import { statSync } from 'node:fs';
import { join } from 'node:path';
import sharp from 'sharp';
import { expect, test } from 'vitest';
import { files } from './illustrations';

const dir = join(process.cwd(), 'ui/src/assets/illustrations');

test('every illustration ships as a small WebP that keeps its transparency', async () => {
  for (const file of Object.values(files)) {
    const path = join(dir, `${file}.webp`);
    expect(statSync(path).size, file).toBeLessThan(300 * 1024);
    const meta = await sharp(path).metadata();
    expect(meta.format, file).toBe('webp');
    expect(meta.hasAlpha, file).toBe(true);
  }
});
