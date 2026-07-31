// Formatting helpers. Chart/tooltip timestamps follow the site convention:
// compact MM/DD HH:MM.

export function compactTimestamp(tsSecondsOrIso: number | string): string {
  const d = typeof tsSecondsOrIso === 'number' ? new Date(tsSecondsOrIso * 1000) : new Date(tsSecondsOrIso)
  if (Number.isNaN(d.getTime())) return '—'
  const mm = String(d.getMonth() + 1).padStart(2, '0')
  const dd = String(d.getDate()).padStart(2, '0')
  const hh = String(d.getHours()).padStart(2, '0')
  const mi = String(d.getMinutes()).padStart(2, '0')
  return `${mm}/${dd} ${hh}:${mi}`
}

export function relativeTime(tsSeconds: number | null | undefined, nowSeconds: number): string {
  if (!tsSeconds) return '—'
  const diff = Math.max(0, nowSeconds - tsSeconds)
  if (diff < 5) return 'just now'
  if (diff < 60) return `${diff}s ago`
  if (diff < 3600) return `${Math.floor(diff / 60)}m ago`
  if (diff < 86400) return `${Math.floor(diff / 3600)}h ${Math.floor((diff % 3600) / 60)}m ago`
  return `${Math.floor(diff / 86400)}d ago`
}

export function relativeIso(iso: string | null | undefined, nowSeconds: number): string {
  if (!iso) return '—'
  const ts = Math.floor(new Date(iso).getTime() / 1000)
  if (Number.isNaN(ts)) return '—'
  return relativeTime(ts, nowSeconds)
}

export function countdown(tsSeconds: number | null | undefined, nowSeconds: number): string {
  if (tsSeconds == null) return '—'
  const diff = tsSeconds - nowSeconds
  if (diff <= 0) return 'now'
  if (diff < 60) return `${diff}s`
  const m = Math.floor(diff / 60)
  const s = diff % 60
  return `${m}m ${String(s).padStart(2, '0')}s`
}

export function intervalLabel(secs: number): string {
  if (secs % 60 === 0) return `${secs / 60} min`
  return `${Math.floor(secs / 60)}m ${secs % 60}s`
}

export function escapeHtml(value: string): string {
  return value
    .replaceAll('&', '&amp;')
    .replaceAll('<', '&lt;')
    .replaceAll('>', '&gt;')
    .replaceAll('"', '&quot;')
    .replaceAll("'", '&#39;')
}

export function statusDotClass(platformStatus: string | null, inService: boolean, halted: boolean): string {
  if (halted) return 'dot dot-err'
  if (!inService) return 'dot dot-off'
  switch (platformStatus) {
    case 'online':
      return 'dot dot-ok'
    case 'idle':
      return 'dot dot-idle'
    case 'offline':
      return 'dot dot-err'
    case 'disabled':
      return 'dot dot-off'
    default:
      return 'dot dot-off'
  }
}
