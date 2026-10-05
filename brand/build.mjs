// Generates every AriadShift brand asset from one geometry definition:
// mark (color + mono), app icons, favicon, outlined wordmark, lockups and a preview board.
// Run with `pnpm build`; outputs land in ./svg, ./png and ./preview.png.
import fs from 'node:fs';
import path from 'node:path';
import { fileURLToPath } from 'node:url';
import { Resvg } from '@resvg/resvg-js';
import opentype from 'opentype.js';
import { optimize } from 'svgo';

const ROOT = path.dirname(fileURLToPath(import.meta.url));
const SVG_DIR = path.join(ROOT, 'svg');
const PNG_DIR = path.join(ROOT, 'png');

// Brand tokens. Contrast targets: thread vs target sheet >= 4.5, thread vs source sheet >= 3.
const COLOR = {
  source: '#7A976B', // raw document (sage 500)
  target: '#4A6940', // converted document (sage 700)
  thread: '#FCFBF6', // Ariadne's thread (paper white)
  paper: '#F6F4EE', // light background
  paperDeep: '#ECE8DD', // app icon gradient end
  ink: '#232821', // wordmark on light
  night: '#171A16', // dark background
};

// Mark geometry on a 256 grid. The two sheets are point-symmetric around (128,128),
// so the S-shaped thread is built as one half plus its 180-degree rotation.
// The source sheet is a plain "raw" page; the target carries a dog-ear as the finished document.
const SOURCE = { x: 24, y: 22, w: 124, h: 156, r: 22, ear: 0 };
const TARGET = { x: 108, y: 78, w: 124, h: 156, r: 22, ear: 38 };
const GAP = 9;
const THREAD_WIDTH = 15;
const S_HALF = [[122, 46], [58, 34], [42, 102], [100, 118]]; // start, ctrl1, ctrl2, spine start

const sheetPath = ({ x, y, w, h, r, ear }, inset = 0) => {
  const [X, Y, W, H, R] = [x - inset, y - inset, w + inset * 2, h + inset * 2, r + inset];
  const topRight = ear ? `H${X + W - ear}L${X + W} ${Y + ear}` : `H${X + W - R}Q${X + W} ${Y} ${X + W} ${Y + R}`;
  return `M${X + R} ${Y}${topRight}V${Y + H - R}Q${X + W} ${Y + H} ${X + W - R} ${Y + H}H${X + R}Q${X} ${Y + H} ${X} ${Y + H - R}V${Y + R}Q${X} ${Y} ${X + R} ${Y}Z`;
};
const earPath = ({ x, y, w, ear }) =>
  `M${x + w - ear} ${y}V${y + ear - 10}Q${x + w - ear} ${y + ear} ${x + w - ear + 10} ${y + ear}H${x + w}Z`;
const rotate = ([x, y]) => [256 - x, 256 - y];
const [P0, C1, C2, P1] = S_HALF;
const [Q1, D2, D1, Q0] = [P1, C2, C1, P0].map(rotate);
const THREAD = `M${P0}C${C1} ${C2} ${P1}L${Q1}C${D2} ${D1} ${Q0}`;
const START = { cx: P0[0], cy: P0[1] };
const END = { cx: Q0[0], cy: Q0[1] };

/** Color mark: source sheet cut by a gap around the target sheet, thread drawn on top. */
function markColor(id = 'm') {
  return `
  <mask id="${id}-gap" maskUnits="userSpaceOnUse" x="0" y="0" width="256" height="256">
    <rect width="256" height="256" fill="#fff"/><path d="${sheetPath(TARGET, GAP)}" fill="#000"/>
  </mask>
  <path d="${sheetPath(SOURCE)}" fill="${COLOR.source}" mask="url(#${id}-gap)"/>
  <path d="${sheetPath(TARGET)}" fill="${COLOR.target}"/>
  <path d="${earPath(TARGET)}" fill="#fff" opacity=".4"/>
  <path d="${THREAD}" fill="none" stroke="${COLOR.thread}" stroke-width="${THREAD_WIDTH}" stroke-linecap="round" stroke-linejoin="round"/>
  <circle cx="${START.cx}" cy="${START.cy}" r="12.5" fill="${COLOR.source}" stroke="${COLOR.thread}" stroke-width="8.5"/>
  <circle cx="${END.cx}" cy="${END.cy}" r="14" fill="${COLOR.thread}"/>`;
}

/** Single-color mark: thread, ring and gap become negative space so it works on any background. */
function markMono(fill = 'currentColor', id = 'mono') {
  return `
  <mask id="${id}-cut" maskUnits="userSpaceOnUse" x="0" y="0" width="256" height="256">
    <rect width="256" height="256" fill="#fff"/>
    <path d="${THREAD}" fill="none" stroke="#000" stroke-width="${THREAD_WIDTH}" stroke-linecap="round" stroke-linejoin="round"/>
    <circle cx="${START.cx}" cy="${START.cy}" r="12.5" fill="none" stroke="#000" stroke-width="8.5"/>
    <circle cx="${END.cx}" cy="${END.cy}" r="14" fill="#000"/>
  </mask>
  <mask id="${id}-gap" maskUnits="userSpaceOnUse" x="0" y="0" width="256" height="256">
    <rect width="256" height="256" fill="#fff"/><path d="${sheetPath(TARGET, GAP)}" fill="#000"/>
  </mask>
  <g fill="${fill}" mask="url(#${id}-cut)">
    <path d="${sheetPath(SOURCE)}" mask="url(#${id}-gap)"/>
    <path d="${sheetPath(TARGET)}"/>
  </g>`;
}

/** Superellipse (n=5) approximates Apple's continuous-corner squircle better than a rounded rect. */
function squircle(cx, cy, half, n = 5, steps = 256) {
  const pts = [];
  for (let i = 0; i < steps; i++) {
    const t = (i / steps) * Math.PI * 2;
    const c = Math.cos(t), s = Math.sin(t);
    pts.push([
      cx + half * Math.sign(c) * Math.abs(c) ** (2 / n),
      cy + half * Math.sign(s) * Math.abs(s) ** (2 / n),
    ]);
  }
  return `M${pts.map(([x, y]) => `${x.toFixed(2)} ${y.toFixed(2)}`).join('L')}Z`;
}

/** App icon on a 1024 canvas. `inset` leaves room for macOS-style margin and shadow. */
function appIcon({ inset = 0, shadow = false } = {}) {
  const half = 512 - inset;
  const markSize = half * 2 * 0.7;
  const offset = 512 - markSize / 2;
  const filter = shadow
    ? `<filter id="drop" x="-20%" y="-20%" width="140%" height="140%"><feDropShadow dx="0" dy="10" stdDeviation="14" flood-color="#000" flood-opacity=".28"/></filter>`
    : '';
  return svgDoc(1024, 1024, `
  <defs>
    <linearGradient id="bg" x1="0" y1="0" x2="0" y2="1">
      <stop offset="0" stop-color="#FDFCF8"/><stop offset="1" stop-color="${COLOR.paperDeep}"/>
    </linearGradient>
    ${filter}
  </defs>
  <path d="${squircle(512, 512, half)}" fill="url(#bg)" ${shadow ? 'filter="url(#drop)"' : ''}/>
  <g transform="translate(${offset} ${offset}) scale(${markSize / 256})">${markColor('app')}</g>`);
}

const svgDoc = (w, h, body, viewBox = `0 0 ${w} ${h}`) =>
  `<svg xmlns="http://www.w3.org/2000/svg" width="${w}" height="${h}" viewBox="${viewBox}" fill="none">${body}</svg>`;

// Wordmark: outlined glyph paths so no font install is needed wherever the logo is used.
const FONT = opentype.parse(
  fs.readFileSync(path.join(ROOT, 'node_modules/@fontsource/plus-jakarta-sans/files/plus-jakarta-sans-latin-700-normal.woff')).buffer,
);
const TRACKING = -0.02; // em

function wordmark(text, size) {
  // Per-glyph layout with kerning: opentype.js shaping trips on this font's GSUB tables.
  const glyphs = [...text].map((ch) => FONT.charToGlyph(ch));
  const scale = size / FONT.unitsPerEm;
  let x = 0;
  let d = '';
  glyphs.forEach((g, i) => {
    d += g.getPath(x, 0, size).toPathData(2);
    const kern = i < glyphs.length - 1 ? FONT.getKerningValue(g, glyphs[i + 1]) : 0;
    x += (g.advanceWidth + kern) * scale + (i < glyphs.length - 1 ? TRACKING * size : 0);
  });
  const capHeight = FONT.tables.os2.sCapHeight * scale;
  return { d, width: x, capHeight };
}

/** Horizontal lockup: mark height 256, cap height ~0.42 of mark, optically centered. */
function lockup(textColor) {
  const size = 150;
  const wm = wordmark('AriadShift', size);
  const gap = 64;
  const baseline = 128 + wm.capHeight / 2;
  const width = Math.ceil(256 + gap + wm.width + 8);
  return svgDoc(width, 256, `
  ${markColor('lk')}
  <path transform="translate(${256 + gap} ${baseline.toFixed(2)})" d="${wm.d}" fill="${textColor}"/>`);
}

function wordmarkOnly(textColor) {
  const size = 150;
  const wm = wordmark('AriadShift', size);
  const h = Math.ceil(wm.capHeight + 48);
  return svgDoc(Math.ceil(wm.width + 8), h, `<path transform="translate(2 ${(h - (h - wm.capHeight) / 2).toFixed(2)})" d="${wm.d}" fill="${textColor}"/>`);
}

// prefixIds keeps mask ids unique when several logos are inlined into one HTML page.
const clean = (svg, name) =>
  optimize(svg, { multipass: true, plugins: ['preset-default', { name: 'prefixIds', params: { prefix: name.replace('.svg', '') } }] }).data;
const render = (svg, width) => new Resvg(svg, { fitTo: { mode: 'width', value: width } }).render().asPng();

fs.mkdirSync(SVG_DIR, { recursive: true });
fs.mkdirSync(PNG_DIR, { recursive: true });

const assets = {
  'ariadshift-mark.svg': svgDoc(256, 256, markColor()),
  'ariadshift-mark-mono.svg': svgDoc(256, 256, markMono()),
  'ariadshift-app-icon.svg': appIcon(),
  'ariadshift-app-icon-macos.svg': appIcon({ inset: 100, shadow: true }),
  'ariadshift-wordmark.svg': wordmarkOnly(COLOR.ink),
  'ariadshift-lockup.svg': lockup(COLOR.ink),
  'ariadshift-lockup-dark.svg': lockup(COLOR.thread),
  'favicon.svg': svgDoc(256, 256, markColor('fav')),
};
for (const [name, svg] of Object.entries(assets)) fs.writeFileSync(path.join(SVG_DIR, name), clean(svg, name));

const pngs = [
  ['ariadshift-app-icon.svg', [1024, 512, 256, 128, 64, 32, 16]],
  ['ariadshift-app-icon-macos.svg', [1024]],
  ['ariadshift-mark.svg', [512, 192, 32, 16]],
  ['ariadshift-lockup.svg', [1200]],
  ['ariadshift-lockup-dark.svg', [1200]],
];
for (const [name, sizes] of pngs) {
  const svg = assets[name];
  for (const w of sizes) fs.writeFileSync(path.join(PNG_DIR, `${name.replace('.svg', '')}-${w}.png`), render(svg, w));
}

// Preview board: true-pixel small sizes are upscaled with nearest-neighbour so blur is honest.
const b64 = (buf) => `data:image/png;base64,${buf.toString('base64')}`;
const tile = (x, y, w, h, fill) => `<rect x="${x}" y="${y}" width="${w}" height="${h}" rx="24" fill="${fill}"/>`;
const img = (x, y, size, buf, pixelated = false) =>
  `<image x="${x}" y="${y}" width="${size}" height="${size}" href="${b64(buf)}"${pixelated ? ' style="image-rendering:pixelated"' : ''}/>`;
const lockLight = render(assets['ariadshift-lockup.svg'], 1100);
const lockDark = render(assets['ariadshift-lockup-dark.svg'], 1100);
const lockH = Math.round((256 / Number(assets['ariadshift-lockup.svg'].match(/width="(\d+)"/)[1])) * 1100);
const board = svgDoc(1600, 1180, `
  <rect width="1600" height="1180" fill="#FFFFFF"/>
  ${tile(40, 40, 520, 520, COLOR.paper)}${img(80, 80, 440, render(assets['ariadshift-app-icon-macos.svg'], 880))}
  ${tile(600, 40, 520, 520, COLOR.night)}${img(700, 140, 320, render(assets['ariadshift-mark.svg'], 640))}
  ${tile(1160, 40, 400, 250, COLOR.paper)}
  ${img(1190, 75, 128, render(assets['ariadshift-mark.svg'], 32), true)}${img(1340, 75, 64, render(assets['ariadshift-mark.svg'], 16), true)}
  ${img(1430, 75, 64, render(assets['ariadshift-app-icon.svg'], 16), true)}
  ${tile(1160, 310, 400, 250, '#FFFFFF')}
  <g transform="translate(1235 335) scale(0.78)" color="${COLOR.target}">${markMono(COLOR.target, 'pv')}</g>
  ${tile(40, 600, 1520, 260, COLOR.paper)}<image x="250" y="${730 - lockH / 2}" width="1100" height="${lockH}" href="${b64(lockLight)}"/>
  ${tile(40, 880, 1520, 260, COLOR.night)}<image x="250" y="${1010 - lockH / 2}" width="1100" height="${lockH}" href="${b64(lockDark)}"/>`);
fs.writeFileSync(path.join(ROOT, 'preview.png'), new Resvg(board).render().asPng());

console.log(`Wrote ${Object.keys(assets).length} SVGs, PNG renders and preview.png`);
