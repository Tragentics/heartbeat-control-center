// Inline SVG icon set — hand-drawn, 24×24 viewBox, 2px round strokes to match
// the site's lucide-style iconography. Kept tiny and dependency-free.

const wrap = (inner: string, cls = ''): string =>
  `<svg class="icon ${cls}" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true">${inner}</svg>`

export const icons = {
  pulse: wrap(
    '<path d="M3 12h4l2.2 -5.4 3.6 10.8 2.2 -5.4h6"/>',
  ),
  heart: wrap(
    '<path d="M19.5 13.1 12 20.5l-7.5 -7.4a5.2 5.2 0 1 1 7.5 -7.2 5.2 5.2 0 1 1 7.5 7.2"/><path d="M8.5 12.5h2l1.5 -3 1.5 4 1 -2h2.5" stroke-width="1.7"/>',
  ),
  activity: wrap('<path d="M4 13h3l2.5 -6 4.5 11 2.5 -5H21"/>'),
  gauge: wrap('<path d="M12 15l4 -4"/><path d="M4.5 16.5a8.5 8.5 0 1 1 15 0"/>'),
  plus: wrap('<path d="M12 5v14M5 12h14"/>'),
  settings: wrap(
    '<circle cx="12" cy="12" r="3.2"/><path d="M12 2.8v2.4M12 18.8v2.4M2.8 12h2.4M18.8 12h2.4M5.5 5.5l1.7 1.7M16.8 16.8l1.7 1.7M18.5 5.5l-1.7 1.7M7.2 16.8l-1.7 1.7"/>',
  ),
  refresh: wrap('<path d="M20 11a8 8 0 1 0 .6 4.5"/><path d="M20 4v7h-7"/>'),
  bolt: wrap('<path d="M13 2 5 13.5h6L11 22l8 -11.5h-6z"/>'),
  trash: wrap('<path d="M4 7h16M9 7V4.8A1 1 0 0 1 10 4h4a1 1 0 0 1 1 1V7M6.5 7l1 13h9l1 -13"/><path d="M10 11v5M14 11v5"/>'),
  x: wrap('<path d="M6 6l12 12M18 6 6 18"/>'),
  chevronRight: wrap('<path d="m9 5 7 7-7 7"/>'),
  lock: wrap('<rect x="5" y="11" width="14" height="9.5" rx="2"/><path d="M8 11V7.5a4 4 0 0 1 8 0V11"/>'),
  unlock: wrap('<rect x="5" y="11" width="14" height="9.5" rx="2"/><path d="M8 11V7.5a4 4 0 0 1 7.7 -1.4"/>'),
  key: wrap('<circle cx="8" cy="14" r="4"/><path d="M11 11 20 2M16.5 5.5 19 8M13.5 8.5 15.5 10.5"/>'),
  shield: wrap('<path d="M12 2.8 5 5.5v5.6c0 4.6 3 8 7 10.1 4 -2.1 7 -5.5 7 -10.1V5.5z"/><path d="m9 11.5 2.2 2.2L15.5 9.4"/>'),
  folder: wrap('<path d="M3.5 6.5A1.5 1.5 0 0 1 5 5h4l2 2.5h8A1.5 1.5 0 0 1 20.5 9v8A1.5 1.5 0 0 1 19 18.5H5A1.5 1.5 0 0 1 3.5 17z"/>'),
  external: wrap('<path d="M14 4h6v6M20 4 11 13"/><path d="M19 14v5a1.5 1.5 0 0 1 -1.5 1.5h-12A1.5 1.5 0 0 1 4 19V6.5A1.5 1.5 0 0 1 5.5 5H10"/>'),
  pause: wrap('<path d="M9 5v14M15 5v14"/>'),
  play: wrap('<path d="M7 4.8v14.4L19 12z"/>'),
  alert: wrap('<path d="M12 3 2.8 19.5h18.4z"/><path d="M12 9.5v4.5M12 16.8v.4"/>'),
  check: wrap('<path d="m5 12.5 4.5 4.5L19 7.5"/>'),
  info: wrap('<circle cx="12" cy="12" r="9"/><path d="M12 10.5V17M12 7.2v.4"/>'),
  wifiOff: wrap('<path d="M4 4l16 16M9.5 9.8A9.8 9.8 0 0 0 5.5 12M2.5 8.5a14 14 0 0 1 4.2 -2.8M12 20h.01M8.8 15.8a5.5 5.5 0 0 1 3.2 -1.3M15.6 12.4A9.8 9.8 0 0 0 12 10.2M21.5 8.5A14 14 0 0 0 12 5"/>'),
  clock: wrap('<circle cx="12" cy="12" r="8.5"/><path d="M12 7.5V12l3 2.5"/>'),
  vault: wrap('<rect x="3.5" y="4.5" width="17" height="15" rx="2"/><circle cx="12" cy="12" r="3.5"/><path d="M12 8.5V12l2.2 1.5M6.5 20v1.5M17.5 20v1.5"/>'),
  eye: wrap('<path d="M2.5 12S6 5.8 12 5.8 21.5 12 21.5 12 18 18.2 12 18.2 2.5 12 2.5 12z"/><circle cx="12" cy="12" r="2.8"/>'),
  power: wrap('<path d="M12 3v8"/><path d="M7 6.3a7.5 7.5 0 1 0 10 0"/>'),
  moon: wrap('<path d="M20 14.5A8.5 8.5 0 0 1 9.5 4 8.5 8.5 0 1 0 20 14.5z"/>'),
  sun: wrap('<circle cx="12" cy="12" r="4"/><path d="M12 2.5v2M12 19.5v2M2.5 12h2M19.5 12h2M5 5l1.4 1.4M17.6 17.6 19 19M19 5l-1.4 1.4M6.4 17.6 5 19"/>'),
  monitor: wrap('<rect x="3" y="4.5" width="18" height="12.5" rx="1.8"/><path d="M9 21h6M12 17.5V21"/>'),
} as const

export type IconName = keyof typeof icons
