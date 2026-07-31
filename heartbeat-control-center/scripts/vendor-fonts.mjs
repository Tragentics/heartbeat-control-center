// Vendors the Geist / Geist Mono variable woff2 files (OFL-licensed) from the
// `geist` npm package into src/assets/fonts so the app is fully self-contained.
// Usage: node scripts/vendor-fonts.mjs

import { copyFile, mkdir, readdir, stat } from 'fs/promises'
import path from 'path'
import { fileURLToPath } from 'url'

const root = path.dirname(path.dirname(fileURLToPath(import.meta.url)))
const pkgDir = path.join(root, 'node_modules', 'geist')
const outDir = path.join(root, 'src', 'assets', 'fonts')

async function* walk(dir) {
  for (const entry of await readdir(dir, { withFileTypes: true })) {
    const p = path.join(dir, entry.name)
    if (entry.isDirectory()) yield* walk(p)
    else yield p
  }
}

await mkdir(outDir, { recursive: true })

const wanted = new Map([
  ['geist', { match: /Geist\[wght\]\.woff2$|Geist-Variable\.woff2$|GeistVF\.woff2$/i, out: 'Geist-Variable.woff2' }],
  ['mono', { match: /GeistMono\[wght\]\.woff2$|GeistMono-Variable\.woff2$|GeistMonoVF\.woff2$/i, out: 'GeistMono-Variable.woff2' }],
])

const found = new Map()
try {
  await stat(pkgDir)
} catch {
  console.error('geist package not installed — run npm install first')
  process.exit(1)
}

for await (const file of walk(pkgDir)) {
  for (const [key, spec] of wanted) {
    if (!found.has(key) && spec.match.test(path.basename(file))) {
      found.set(key, file)
    }
  }
  if (/^(LICENSE|OFL)(\.txt|\.md)?$/i.test(path.basename(file)) && !found.has('license')) {
    found.set('license', file)
  }
}

const sans = found.get('geist')
const mono = found.get('mono')
if (!sans || !mono) {
  console.error('Could not locate Geist variable woff2 files in the geist package.')
  console.error('Found so far:', Object.fromEntries(found))
  process.exit(1)
}

await copyFile(sans, path.join(outDir, 'Geist-Variable.woff2'))
await copyFile(mono, path.join(outDir, 'GeistMono-Variable.woff2'))
console.log('vendored Geist-Variable.woff2 + GeistMono-Variable.woff2')
if (found.has('license')) {
  await copyFile(found.get('license'), path.join(outDir, 'OFL-LICENSE.txt'))
  console.log('vendored font license')
}
