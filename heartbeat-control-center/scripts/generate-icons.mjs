// Generates the app icon (1024px master) and the four status tray icons.
// Run `npx tauri icon src-tauri/icons/app-icon.png` afterwards to produce the
// full multi-size set (icon.ico, icon.icns, 32x32.png, ...).
// Usage: node scripts/generate-icons.mjs

import { createRequire } from 'module'
import { mkdir } from 'fs/promises'
import path from 'path'
import { fileURLToPath } from 'url'

const require = createRequire(import.meta.url)
const sharp = require('sharp')

const root = path.dirname(path.dirname(fileURLToPath(import.meta.url)))
const iconsDir = path.join(root, 'src-tauri', 'icons')

// Brand mark: dark rounded square (site dark card tones), emerald pulse line,
// chart-blue terminal dot. Reads cleanly from 16px to 1024px.
const appIconSvg = `<svg width="1024" height="1024" viewBox="0 0 1024 1024" xmlns="http://www.w3.org/2000/svg">
  <defs>
    <linearGradient id="bg" x1="0" y1="0" x2="1" y2="1">
      <stop offset="0" stop-color="#2c2c2e"/>
      <stop offset="1" stop-color="#161618"/>
    </linearGradient>
    <linearGradient id="edge" x1="0" y1="0" x2="0" y2="1">
      <stop offset="0" stop-color="#ffffff" stop-opacity="0.16"/>
      <stop offset="1" stop-color="#ffffff" stop-opacity="0.03"/>
    </linearGradient>
  </defs>
  <rect x="32" y="32" width="960" height="960" rx="212" fill="url(#bg)"/>
  <rect x="44" y="44" width="936" height="936" rx="200" fill="none" stroke="url(#edge)" stroke-width="24"/>
  <path d="M 148 512 H 348 L 442 300 L 588 724 L 682 512 H 800"
        fill="none" stroke="#10b981" stroke-width="76"
        stroke-linecap="round" stroke-linejoin="round"/>
  <circle cx="856" cy="512" r="46" fill="#3b82f6"/>
</svg>`

const trayPulse = (color) => `<svg width="128" height="128" viewBox="0 0 128 128" xmlns="http://www.w3.org/2000/svg">
  <path d="M 8 64 H 38 L 52 30 L 74 96 L 88 64 H 106"
        fill="none" stroke="${color}" stroke-width="13"
        stroke-linecap="round" stroke-linejoin="round"/>
  <circle cx="118" cy="64" r="9" fill="${color}"/>
</svg>`

const TRAY = {
  'tray-ok.png': '#10b981',
  'tray-warn.png': '#f59e0b',
  'tray-err.png': '#ef4444',
  'tray-paused.png': '#9ca3af',
}

await mkdir(iconsDir, { recursive: true })

await sharp(Buffer.from(appIconSvg)).png({ compressionLevel: 9 }).toFile(path.join(iconsDir, 'app-icon.png'))
console.log('wrote src-tauri/icons/app-icon.png (1024x1024 master)')

for (const [name, color] of Object.entries(TRAY)) {
  await sharp(Buffer.from(trayPulse(color)))
    .resize(32, 32)
    .png({ compressionLevel: 9 })
    .toFile(path.join(iconsDir, name))
  console.log(`wrote src-tauri/icons/${name}`)
}
