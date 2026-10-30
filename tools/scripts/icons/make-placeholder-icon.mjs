#!/usr/bin/env node
// Writes a placeholder 1024x1024 app icon (a film frame on a dark tile) as a PNG.
// Usage: node tools/scripts/icons/make-placeholder-icon.mjs <out.png>
// Then:  pnpm --filter @gtp/desktop exec tauri icon src-tauri/icons/source.png
// Replace with real artwork later.
import { writeFileSync } from 'node:fs';
import { deflateSync } from 'node:zlib';

const SIZE = 1024;
const out = process.argv[2];
if (!out) {
  console.error('usage: make-placeholder-icon.mjs <out.png>');
  process.exit(2);
}

const crcTable = new Uint32Array(256).map((_, n) => {
  let c = n;
  for (let k = 0; k < 8; k++) c = c & 1 ? 0xedb88320 ^ (c >>> 1) : c >>> 1;
  return c >>> 0;
});
const crc32 = (buf) => {
  let c = 0xffffffff;
  for (const b of buf) c = crcTable[(c ^ b) & 0xff] ^ (c >>> 8);
  return (c ^ 0xffffffff) >>> 0;
};
const chunk = (type, data) => {
  const len = Buffer.alloc(4);
  len.writeUInt32BE(data.length);
  const body = Buffer.concat([Buffer.from(type), data]);
  const crc = Buffer.alloc(4);
  crc.writeUInt32BE(crc32(body));
  return Buffer.concat([len, body, crc]);
};

const tile = { r: 28, g: 28, b: 30 };
const frameLight = { r: 230, g: 230, b: 232 };
const radius = 180;
const frame = { x0: 162, x1: 862, y0: 281, y1: 743, border: 44 };

function pixel(x, y) {
  // rounded tile with transparent corners
  const cx = Math.min(Math.max(x, radius), SIZE - 1 - radius);
  const cy = Math.min(Math.max(y, radius), SIZE - 1 - radius);
  if ((x - cx) ** 2 + (y - cy) ** 2 > radius ** 2) return [0, 0, 0, 0];

  const inFrame = x >= frame.x0 && x < frame.x1 && y >= frame.y0 && y < frame.y1;
  if (inFrame) {
    const inner =
      x >= frame.x0 + frame.border &&
      x < frame.x1 - frame.border &&
      y >= frame.y0 + frame.border &&
      y < frame.y1 - frame.border;
    if (!inner) return [frameLight.r, frameLight.g, frameLight.b, 255];
    const t = (x - frame.x0) / (frame.x1 - frame.x0); // grain-to-pixel: dark to light
    const v = Math.round(58 + t * 120);
    return [v, v, v + 4, 255];
  }
  // film perforations above and below the frame
  const perfRow = (y >= 150 && y < 210) || (y >= 814 && y < 874);
  if (perfRow && x >= 162 && x < 862) {
    const i = Math.floor((x - 162) / 100);
    const within = (x - 162) % 100;
    if (i < 7 && within >= 20 && within < 80)
      return [frameLight.r, frameLight.g, frameLight.b, 255];
  }
  return [tile.r, tile.g, tile.b, 255];
}

const raw = Buffer.alloc((SIZE * 4 + 1) * SIZE);
for (let y = 0; y < SIZE; y++) {
  const row = y * (SIZE * 4 + 1);
  raw[row] = 0; // filter: none
  for (let x = 0; x < SIZE; x++) {
    const [r, g, b, a] = pixel(x, y);
    const o = row + 1 + x * 4;
    raw[o] = r;
    raw[o + 1] = g;
    raw[o + 2] = b;
    raw[o + 3] = a;
  }
}

const ihdr = Buffer.alloc(13);
ihdr.writeUInt32BE(SIZE, 0);
ihdr.writeUInt32BE(SIZE, 4);
ihdr[8] = 8; // bit depth
ihdr[9] = 6; // RGBA
const png = Buffer.concat([
  Buffer.from([0x89, 0x50, 0x4e, 0x47, 0x0d, 0x0a, 0x1a, 0x0a]),
  chunk('IHDR', ihdr),
  chunk('IDAT', deflateSync(raw, { level: 9 })),
  chunk('IEND', Buffer.alloc(0)),
]);
writeFileSync(out, png);
console.log(`wrote ${out} (${png.length} bytes)`);
