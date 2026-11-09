import { expect, test } from 'vitest';
import { shownSites, siteUrl } from './tray';

test('a site opens with the scheme and port Stoke serves it on', () => {
  expect(siteUrl({ name: 'blog', secured: false }, 80, 443)).toBe('http://blog.test');
  expect(siteUrl({ name: 'shop', secured: true }, 8080, 8443)).toBe('https://shop.test:8443');
  expect(siteUrl({ name: 'shop', secured: false }, 8080, 8443)).toBe('http://shop.test:8080');
  expect(siteUrl({ name: 'shop', secured: true }, 80, 443)).toBe('https://shop.test');
});

test('the panel shows the first five sites by name and counts all', () => {
  const sites = Array.from({ length: 40 }, (_, i) => ({ name: `site${String(40 - i).padStart(2, '0')}` }));
  const { shown, more } = shownSites(sites);
  expect(shown.map((s) => s.name)).toEqual(['site01', 'site02', 'site03', 'site04', 'site05']);
  expect(more).toBe(40);
  expect(shownSites(sites.slice(0, 3)).more).toBe(0);
});
