// Liquid Glass rendering of the AriadShift app icon: frosted glass sheets that refract
// what lies behind them, specular rims, layered shadows and a golden glass thread.
import {
  BEAD_R, COLOR, END, RING, SOURCE, START, TARGET, THREAD, THREAD_WIDTH, earPath, sheetPath, squircle, svgDoc,
} from './geometry.mjs';

const APPEARANCE = {
  light: { bg: COLOR.bg, bloom: 0.38, glassA: [0.36, 0.1], glassB: [0.44, 0.12], shadow: 0.4, shadowColor: COLOR.shadow },
  dark: { bg: COLOR.bgDark, bloom: 0.2, glassA: [0.22, 0.06], glassB: [0.3, 0.09], shadow: 0.55, shadowColor: '#000000' },
};

const SIZE = 1024;
const MARK_SCALE = 0.74; // mark width relative to the squircle

/**
 * @param {{ appearance?: 'light' | 'dark', inset?: number, outerShadow?: boolean }} options
 *   inset leaves a margin around the squircle (macOS template uses 100 on a 1024 canvas).
 */
export function glassIcon({ appearance = 'light', inset = 0, outerShadow = false } = {}) {
  const a = APPEARANCE[appearance];
  const half = SIZE / 2 - inset;
  const k = (half * 2 * MARK_SCALE) / 256;
  const off = SIZE / 2 - 128 * k;
  const mark = `translate(${off.toFixed(2)} ${off.toFixed(2)}) scale(${k.toFixed(4)})`;
  const box = `x="${SIZE / 2 - half}" y="${SIZE / 2 - half}" width="${half * 2}" height="${half * 2}"`;
  const ringHole = RING.r - RING.stroke / 2 + 0.4;

  const defs = `
  <defs>
    <linearGradient id="bg" x1=".1" y1="0" x2=".9" y2="1">
      <stop offset="0" stop-color="${a.bg[0]}"/><stop offset=".55" stop-color="${a.bg[1]}"/><stop offset="1" stop-color="${a.bg[2]}"/>
    </linearGradient>
    <radialGradient id="bloom" cx=".28" cy=".18" r=".75">
      <stop offset="0" stop-color="#fff" stop-opacity="${a.bloom}"/><stop offset="1" stop-color="#fff" stop-opacity="0"/>
    </radialGradient>
    <linearGradient id="iconRim" x1="0" y1="0" x2="0" y2="1">
      <stop offset="0" stop-color="#fff" stop-opacity=".5"/><stop offset=".5" stop-color="#fff" stop-opacity=".06"/><stop offset="1" stop-color="#fff" stop-opacity=".18"/>
    </linearGradient>
    <linearGradient id="glassA" x1="0" y1="0" x2=".7" y2="1">
      <stop offset="0" stop-color="${COLOR.glass}" stop-opacity="${a.glassA[0]}"/><stop offset="1" stop-color="${COLOR.glass}" stop-opacity="${a.glassA[1]}"/>
    </linearGradient>
    <linearGradient id="glassB" x1="0" y1="0" x2=".7" y2="1">
      <stop offset="0" stop-color="${COLOR.glass}" stop-opacity="${a.glassB[0]}"/><stop offset="1" stop-color="${COLOR.glass}" stop-opacity="${a.glassB[1]}"/>
    </linearGradient>
    <linearGradient id="rim" x1="0" y1="0" x2="1" y2="1">
      <stop offset="0" stop-color="#fff" stop-opacity=".95"/><stop offset=".45" stop-color="#fff" stop-opacity=".12"/>
      <stop offset=".8" stop-color="#fff" stop-opacity=".05"/><stop offset="1" stop-color="#fff" stop-opacity=".45"/>
    </linearGradient>
    <linearGradient id="edge" x1="0" y1="0" x2="1" y2="1">
      <stop offset=".55" stop-color="#000" stop-opacity="0"/><stop offset="1" stop-color="#000" stop-opacity=".22"/>
    </linearGradient>
    <linearGradient id="ear" x1="0" y1="1" x2="1" y2="0">
      <stop offset="0" stop-color="#fff" stop-opacity=".95"/><stop offset="1" stop-color="#fff" stop-opacity=".45"/>
    </linearGradient>
    <linearGradient id="gold" gradientUnits="userSpaceOnUse" x1="40" y1="40" x2="216" y2="216">
      <stop offset="0" stop-color="${COLOR.gold[0]}"/><stop offset=".5" stop-color="${COLOR.gold[1]}"/><stop offset="1" stop-color="${COLOR.gold[2]}"/>
    </linearGradient>
    <radialGradient id="bead" cx=".35" cy=".3" r=".75">
      <stop offset="0" stop-color="#fff"/><stop offset=".45" stop-color="${COLOR.gold[1]}"/><stop offset="1" stop-color="${COLOR.gold[2]}"/>
    </radialGradient>
    <clipPath id="icon"><path d="${squircle(SIZE / 2, SIZE / 2, half)}"/></clipPath>
    <clipPath id="front"><path transform="${mark}" d="${sheetPath(TARGET)}"/></clipPath>
    <clipPath id="backInner"><path d="${sheetPath(SOURCE)}"/></clipPath>
    <clipPath id="frontInner"><path d="${sheetPath(TARGET)}"/></clipPath>
    <mask id="tube" maskUnits="userSpaceOnUse" x="-20" y="-20" width="296" height="296">
      <path d="${THREAD}" stroke="#fff" stroke-width="${THREAD_WIDTH}" stroke-linecap="round"/>
      <circle cx="${START.cx}" cy="${START.cy}" r="${RING.r}" stroke="#fff" stroke-width="${RING.stroke}"/>
      <circle cx="${START.cx}" cy="${START.cy}" r="${ringHole}" fill="#000"/>
    </mask>
    <mask id="ringHole" maskUnits="userSpaceOnUse" x="-20" y="-20" width="296" height="296">
      <rect x="-20" y="-20" width="296" height="296" fill="#fff"/>
      <circle cx="${START.cx}" cy="${START.cy}" r="${ringHole}" fill="#000"/>
    </mask>
    <filter id="drop" x="-30%" y="-30%" width="160%" height="170%">
      <feGaussianBlur stdDeviation="7"/><feOffset dy="9"/>
      <feComponentTransfer><feFuncA type="linear" slope="${a.shadow}"/></feComponentTransfer>
    </filter>
    <filter id="frost" x="0" y="0" width="100%" height="100%"><feGaussianBlur stdDeviation="${(6 * k).toFixed(1)}"/></filter>
    <filter id="glow" x="-20%" y="-20%" width="140%" height="140%"><feGaussianBlur stdDeviation="5"/></filter>
    <filter id="contour" x="-10%" y="-10%" width="120%" height="120%"><feGaussianBlur stdDeviation="1.2"/></filter>
    <filter id="lift" x="-20%" y="-20%" width="140%" height="150%"><feGaussianBlur stdDeviation="3"/><feOffset dy="4"/></filter>
    <filter id="outer" x="-15%" y="-15%" width="130%" height="135%">
      <feDropShadow dx="0" dy="12" stdDeviation="16" flood-color="#000" flood-opacity=".3"/>
    </filter>
  </defs>`;

  const background = `
  <g clip-path="url(#icon)">
    <rect ${box} fill="url(#bg)"/>
    <rect ${box} fill="url(#bloom)"/>
  </g>`;

  const glassSheet = (sheet, fill, clip) => `
    <path d="${sheetPath(sheet)}" fill="url(#${fill})"/>
    <path d="${sheetPath(sheet)}" stroke="url(#rim)" stroke-width="1.6"/>
    <path d="${sheetPath(sheet)}" stroke="url(#edge)" stroke-width="3" transform="translate(-.8 -.8)" clip-path="url(#${clip})"/>`;

  // The thread starts at the ring centre, so every thread layer hides the ring's hole.
  const thread = `
    <g mask="url(#ringHole)">
      <path d="${THREAD}" stroke="${a.shadowColor}" stroke-opacity=".45" stroke-width="${THREAD_WIDTH}" stroke-linecap="round" filter="url(#lift)"/>
      <path d="${THREAD}" stroke="${COLOR.goldGlow}" stroke-opacity=".55" stroke-width="18" stroke-linecap="round" filter="url(#glow)"/>
      <path d="${THREAD}" stroke="${a.shadowColor}" stroke-opacity=".42" stroke-width="${THREAD_WIDTH + 4}" stroke-linecap="round" filter="url(#contour)"/>
      <path d="${THREAD}" stroke="url(#gold)" stroke-width="${THREAD_WIDTH}" stroke-linecap="round"/>
    </g>
    <circle cx="${START.cx}" cy="${START.cy}" r="${RING.r}" stroke="${a.shadowColor}" stroke-opacity=".42" stroke-width="${RING.stroke + 4}" filter="url(#contour)"/>
    <circle cx="${START.cx}" cy="${START.cy}" r="${RING.r}" stroke="url(#gold)" stroke-width="${RING.stroke}"/>
    <g mask="url(#tube)">
      <path d="${THREAD}" stroke="#fff" stroke-opacity=".8" stroke-width="4.5" stroke-linecap="round" transform="translate(-2.2 -3)"/>
      <path d="${THREAD}" stroke="#7A4A0A" stroke-opacity=".16" stroke-width="4.5" stroke-linecap="round" transform="translate(2 3.4)"/>
      <circle cx="${START.cx}" cy="${START.cy}" r="${RING.r}" stroke="#fff" stroke-opacity=".75" stroke-width="3" transform="translate(-1.6 -2.2)"/>
    </g>
    <circle cx="${END.cx}" cy="${END.cy + 3}" r="${BEAD_R - 0.5}" fill="${a.shadowColor}" opacity=".35" filter="url(#glow)"/>
    <circle cx="${END.cx}" cy="${END.cy}" r="${BEAD_R + 2}" fill="${a.shadowColor}" opacity=".42" filter="url(#contour)"/>
    <circle cx="${END.cx}" cy="${END.cy}" r="${BEAD_R}" fill="url(#bead)"/>
    <ellipse cx="${END.cx - 4.5}" cy="${END.cy - 5.5}" rx="4.5" ry="3" fill="#fff" opacity=".9"/>`;

  const body = `${defs}
  <g${outerShadow ? ' filter="url(#outer)"' : ''}>
    ${background}
    <path d="${squircle(SIZE / 2, SIZE / 2, half - 1.5)}" stroke="url(#iconRim)" stroke-width="3"/>
  </g>
  <g transform="${mark}">
    <path d="${sheetPath(SOURCE)}" fill="${a.shadowColor}" filter="url(#drop)"/>
    ${glassSheet(SOURCE, 'glassA', 'backInner')}
    <path d="${sheetPath(TARGET)}" fill="${a.shadowColor}" filter="url(#drop)"/>
  </g>
  <g clip-path="url(#front)">
    <g filter="url(#frost)">
      <rect ${box} fill="url(#bg)"/>
      <rect ${box} fill="url(#bloom)"/>
      <g transform="${mark}">
        <path d="${sheetPath(SOURCE)}" fill="url(#glassA)"/>
        <path d="${sheetPath(SOURCE)}" stroke="#fff" stroke-opacity=".35" stroke-width="2"/>
      </g>
    </g>
  </g>
  <g transform="${mark}">
    ${glassSheet(TARGET, 'glassB', 'frontInner')}
    <path d="${earPath(TARGET)}" fill="#000" opacity=".18" filter="url(#lift)"/>
    <path d="${earPath(TARGET)}" fill="url(#ear)"/>
    ${thread}
  </g>`;
  return svgDoc(SIZE, SIZE, body);
}
