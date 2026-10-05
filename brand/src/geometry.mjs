// Shared AriadShift geometry and color tokens on a 256 grid.
// The two sheets are point-symmetric around (128,128), so the S-shaped thread
// is one cubic half plus its 180-degree rotation.

export const COLOR = {
  // App icon background (light / default appearance), top-left to bottom-right.
  bg: ['#A8CE94', '#4F8C62', '#143A2B'],
  // Dark appearance background.
  bgDark: ['#355B44', '#18301F', '#070D0A'],
  glass: '#F4FFF2', // frosted glass tint
  gold: ['#FFF1C4', '#F2C063', '#D8912C'], // Ariadne's golden thread: light, mid, deep
  goldGlow: '#FFD27A',
  shadow: '#0A2216',
  ink: '#232821', // wordmark on light
  paper: '#F6F4EE', // light page background
  night: '#171A16', // dark page background
  mono: '#2F5B43', // single-color mark default
};

// The source sheet is a plain "raw" page; the target carries a dog-ear as the finished document.
export const SOURCE = { x: 24, y: 22, w: 124, h: 156, r: 22, ear: 0 };
export const TARGET = { x: 108, y: 78, w: 124, h: 156, r: 22, ear: 38 };
export const THREAD_WIDTH = 15;
export const RING = { r: 12.5, stroke: 8.5 };
export const BEAD_R = 14.5;

const S_HALF = [[122, 46], [58, 34], [42, 102], [100, 118]]; // start, ctrl1, ctrl2, spine start
const rotate = ([x, y]) => [256 - x, 256 - y];
const [P0, C1, C2, P1] = S_HALF;
const [Q1, D2, D1, Q0] = [P1, C2, C1, P0].map(rotate);
export const THREAD = `M${P0}C${C1} ${C2} ${P1}L${Q1}C${D2} ${D1} ${Q0}`;
export const START = { cx: P0[0], cy: P0[1] };
export const END = { cx: Q0[0], cy: Q0[1] };

export const sheetPath = ({ x, y, w, h, r, ear }, inset = 0) => {
  const [X, Y, W, H, R] = [x - inset, y - inset, w + inset * 2, h + inset * 2, r + inset];
  const topRight = ear ? `H${X + W - ear}L${X + W} ${Y + ear}` : `H${X + W - R}Q${X + W} ${Y} ${X + W} ${Y + R}`;
  return `M${X + R} ${Y}${topRight}V${Y + H - R}Q${X + W} ${Y + H} ${X + W - R} ${Y + H}H${X + R}Q${X} ${Y + H} ${X} ${Y + H - R}V${Y + R}Q${X} ${Y} ${X + R} ${Y}Z`;
};

/** The folded corner facet of a dog-eared sheet. */
export const earPath = ({ x, y, w, ear }) =>
  `M${x + w - ear} ${y}V${y + ear - 9}Q${x + w - ear} ${y + ear} ${x + w - ear + 9} ${y + ear}H${x + w}Z`;

/** Superellipse (n=5) approximates Apple's continuous-corner squircle better than a rounded rect. */
export function squircle(cx, cy, half, n = 5, steps = 256) {
  const pts = [];
  for (let i = 0; i < steps; i++) {
    const t = (i / steps) * Math.PI * 2;
    const c = Math.cos(t);
    const s = Math.sin(t);
    pts.push(`${(cx + half * Math.sign(c) * Math.abs(c) ** (2 / n)).toFixed(2)} ${(cy + half * Math.sign(s) * Math.abs(s) ** (2 / n)).toFixed(2)}`);
  }
  return `M${pts.join('L')}Z`;
}

export const svgDoc = (w, h, body) =>
  `<svg xmlns="http://www.w3.org/2000/svg" width="${w}" height="${h}" viewBox="0 0 ${w} ${h}" fill="none">${body}</svg>`;
