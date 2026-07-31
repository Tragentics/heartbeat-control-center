// Shared render helpers. All render functions return HTML strings; interaction
// is wired via event delegation in main.ts using data-action attributes.

import { escapeHtml } from '../lib/format'
import { icons } from '../lib/icons'
import type { AgentView, BeatRecord, SparkPoint } from '../lib/types'

export const CHART_BLUE = '#3b82f6'
export const C_OK = '#10b981'
export const C_WARN = '#f59e0b'
export const C_ERR = '#ef4444'

export function statusBadge(agent: AgentView): string {
  if (agent.halted) return `<span class="badge badge-offline">halted</span>`
  if (!agent.in_service) return `<span class="badge badge-muted">out of service</span>`
  switch (agent.platform_status) {
    case 'online':
      return `<span class="badge badge-online">online</span>`
    case 'idle':
      return `<span class="badge badge-idle">idle</span>`
    case 'offline':
      return `<span class="badge badge-offline">offline</span>`
    case 'disabled':
      return `<span class="badge badge-offline">disabled</span>`
    default:
      return `<span class="badge badge-muted">unknown</span>`
  }
}

export function switchControl(action: string, id: string, checked: boolean, title: string): string {
  return `<label class="switch" title="${escapeHtml(title)}">
    <input type="checkbox" data-action="${action}" data-id="${escapeHtml(id)}" ${checked ? 'checked' : ''} />
    <span class="track"></span><span class="thumb"></span>
  </label>`
}

/** Mini sparkline: one 3px bar per beat, green ok / amber honest-offline / red fail. */
export function sparkline(points: SparkPoint[], width = 96, height = 26): string {
  if (points.length === 0) {
    return `<svg class="spark" width="${width}" height="${height}"><line x1="0" y1="${height / 2}" x2="${width}" y2="${height / 2}" stroke="var(--border)" stroke-width="1"/></svg>`
  }
  const n = 30
  const slot = width / n
  const bw = Math.max(2, slot - 1.4)
  const max = Math.max(...points.map((p) => p.latency_ms ?? 0), 1)
  const bars = points
    .map((p, i) => {
      const idx = n - points.length + i
      const x = idx * slot
      const h = p.ok ? Math.max(4, ((p.latency_ms ?? max * 0.3) / max) * (height - 4)) : height - 4
      const y = height - h
      const fill = p.ok ? CHART_BLUE : C_ERR
      const opacity = p.ok ? 0.75 : 0.95
      return `<rect x="${x.toFixed(1)}" y="${y.toFixed(1)}" width="${bw.toFixed(1)}" height="${h.toFixed(1)}" rx="1" fill="${fill}" opacity="${opacity}"/>`
    })
    .join('')
  return `<svg class="spark" width="${width}" height="${height}" aria-label="recent beats">${bars}</svg>`
}

export interface Bucket {
  start: number
  ok: number
  fail: number
  latencySum: number
  latencyN: number
}

export function bucketize(records: BeatRecord[], nowSec: number, windowSec: number, buckets: number): Bucket[] {
  const width = windowSec / buckets
  const start = nowSec - windowSec
  const out: Bucket[] = Array.from({ length: buckets }, (_, i) => ({
    start: start + i * width,
    ok: 0,
    fail: 0,
    latencySum: 0,
    latencyN: 0,
  }))
  for (const r of records) {
    if (r.ts < start || r.ts > nowSec) continue
    const idx = Math.min(buckets - 1, Math.floor((r.ts - start) / width))
    const b = out[idx]
    if (!b) continue
    const success = r.kind === 'online_ok' || r.kind === 'offline_ok' || r.kind === 'local_fail_offline'
    if (success) b.ok++
    else b.fail++
    if (typeof r.latency_ms === 'number') {
      b.latencySum += r.latency_ms
      b.latencyN++
    }
  }
  return out
}

/** Stacked delivery bars (ok = chart blue, fail = red) with hover tooltips. */
export function deliveryChart(buckets: Bucket[], width = 1000, height = 120): string {
  const maxCount = Math.max(1, ...buckets.map((b) => b.ok + b.fail))
  const slot = width / buckets.length
  const bw = Math.max(3, slot - 2.5)
  const bars = buckets
    .map((b, i) => {
      const total = b.ok + b.fail
      const x = i * slot + (slot - bw) / 2
      if (total === 0) {
        return `<rect x="${x.toFixed(1)}" y="${height - 2}" width="${bw.toFixed(1)}" height="2" rx="1" fill="var(--border)" data-bi="${i}"/>`
      }
      const hTotal = Math.max(3, (total / maxCount) * (height - 8))
      const hFail = b.fail > 0 ? Math.max(2, (b.fail / total) * hTotal) : 0
      const hOk = hTotal - hFail
      const yOk = height - hOk
      const yFail = yOk - hFail
      const okRect = hOk > 0 ? `<rect x="${x.toFixed(1)}" y="${yOk.toFixed(1)}" width="${bw.toFixed(1)}" height="${hOk.toFixed(1)}" rx="1.5" fill="${CHART_BLUE}" opacity="0.8" data-bi="${i}"/>` : ''
      const failRect = hFail > 0 ? `<rect x="${x.toFixed(1)}" y="${yFail.toFixed(1)}" width="${bw.toFixed(1)}" height="${hFail.toFixed(1)}" rx="1.5" fill="${C_ERR}" opacity="0.9" data-bi="${i}"/>` : ''
      return okRect + failRect
    })
    .join('')
  return `<svg class="chart" data-chart="delivery" viewBox="0 0 ${width} ${height}" preserveAspectRatio="none" style="width:100%;height:${height}px;display:block">${bars}</svg>`
}

/** Latency area chart over buckets (avg per bucket). */
export function latencyChart(buckets: Bucket[], width = 1000, height = 110): string {
  const avgs = buckets.map((b) => (b.latencyN > 0 ? b.latencySum / b.latencyN : null))
  const known = avgs.filter((a): a is number => a != null)
  if (known.length === 0) {
    return `<div class="card-sub" style="padding:18px 0;text-align:center">No latency samples in this window yet.</div>`
  }
  const max = Math.max(...known) * 1.15
  const slot = width / buckets.length
  let path = ''
  let area = ''
  let started = false
  const pts: Array<{ x: number; y: number; i: number } | null> = avgs.map((a, i) => {
    if (a == null) return null
    const x = i * slot + slot / 2
    const y = height - 6 - (a / max) * (height - 16)
    return { x, y, i }
  })
  for (const p of pts) {
    if (!p) continue
    path += started ? ` L ${p.x.toFixed(1)} ${p.y.toFixed(1)}` : `M ${p.x.toFixed(1)} ${p.y.toFixed(1)}`
    started = true
  }
  const first = pts.find((p) => p != null)
  const last = [...pts].reverse().find((p) => p != null)
  if (first && last) {
    area = `${path} L ${last.x.toFixed(1)} ${height - 2} L ${first.x.toFixed(1)} ${height - 2} Z`
  }
  const dots = pts
    .filter((p): p is { x: number; y: number; i: number } => p != null)
    .map((p) => `<circle cx="${p.x.toFixed(1)}" cy="${p.y.toFixed(1)}" r="6" fill="transparent" data-bi="${p.i}"/>`)
    .join('')
  return `<svg class="chart" data-chart="latency" viewBox="0 0 ${width} ${height}" preserveAspectRatio="none" style="width:100%;height:${height}px;display:block">
    <path d="${area}" fill="${CHART_BLUE}" opacity="0.14"/>
    <path d="${path}" fill="none" stroke="${CHART_BLUE}" stroke-width="2" stroke-linejoin="round" stroke-linecap="round"/>
    ${dots}
  </svg>`
}

/** Status timeline strip from beat records: contiguous colored segments. */
export function timelineStrip(records: BeatRecord[], nowSec: number, windowSec: number): string {
  const start = nowSec - windowSec
  const relevant = records.filter((r) => r.ts >= start)
  if (relevant.length === 0) {
    return `<div class="timeline-strip"><div class="seg" style="flex:1;background:color-mix(in oklab, var(--muted-foreground) 18%, transparent)"></div></div>`
  }
  type Seg = { from: number; to: number; color: string }
  const colorFor = (r: BeatRecord): string => {
    if (r.kind === 'online_ok') return C_OK
    if (r.kind === 'offline_ok') return 'color-mix(in oklab, var(--muted-foreground) 40%, transparent)'
    if (r.kind === 'local_fail_offline') return C_WARN
    return C_ERR
  }
  const segs: Seg[] = []
  let prevTs = start
  let prevColor = 'color-mix(in oklab, var(--muted-foreground) 18%, transparent)'
  for (const r of relevant) {
    if (r.ts > prevTs) segs.push({ from: prevTs, to: r.ts, color: prevColor })
    prevTs = r.ts
    prevColor = colorFor(r)
  }
  segs.push({ from: prevTs, to: nowSec, color: prevColor })
  const total = windowSec
  const html = segs
    .filter((s) => s.to > s.from)
    .map((s) => `<div class="seg" style="width:${(((s.to - s.from) / total) * 100).toFixed(2)}%;background:${s.color}"></div>`)
    .join('')
  return `<div class="timeline-strip">${html}</div>`
}

export function iconButton(action: string, id: string, icon: keyof typeof icons, title: string, extraClass = ''): string {
  return `<button class="btn-icon ${extraClass}" data-action="${action}" data-id="${escapeHtml(id)}" title="${escapeHtml(title)}" aria-label="${escapeHtml(title)}">${icons[icon]}</button>`
}
