// Health tab — fleet KPIs, delivery + latency charts, status timeline, activity.

import { compactTimestamp, escapeHtml } from '../lib/format'
import { icons } from '../lib/icons'
import type { BeatRecord, Snapshot } from '../lib/types'
import { bucketize, deliveryChart, latencyChart, timelineStrip, type Bucket } from './components'

export type HealthWindow = '24h' | '7d'

export interface HealthModel {
  selected: string // 'fleet' or agent id
  window: HealthWindow
  records: BeatRecord[] // merged records for the selection
  loading: boolean
  /** Activity feed: 1-based page and agent filter ('all' | 'system' | agent id). */
  activityPage: number
  activityAgent: string
}

export const ACTIVITY_PER_PAGE = 10

export function windowSecs(w: HealthWindow): number {
  return w === '24h' ? 24 * 3600 : 7 * 24 * 3600
}

export function bucketCount(w: HealthWindow): number {
  return w === '24h' ? 48 : 84
}

export function renderHealth(snap: Snapshot, model: HealthModel): string {
  const inService = snap.agents.filter((a) => a.in_service).length
  const issues = snap.agents.filter((a) => a.halted || a.consecutive_failures > 0 || a.local_check_failing).length
  const forFleet = model.selected === 'fleet'
  const agent = forFleet ? null : snap.agents.find((a) => a.id === model.selected)

  const stats = (() => {
    if (agent) return model.window === '24h' ? agent.stats_24h : agent.stats_7d
    // Fleet aggregate from per-agent stats.
    const per = snap.agents.map((a) => (model.window === '24h' ? a.stats_24h : a.stats_7d))
    const total = per.reduce((s, x) => s + x.total, 0)
    const ok = per.reduce((s, x) => s + x.ok, 0)
    const latAgents = per.filter((x) => x.avg_latency_ms > 0)
    return {
      total,
      ok,
      success_rate: total > 0 ? Math.round((ok / total) * 100) : 0,
      avg_latency_ms:
        latAgents.length > 0 ? Math.round(latAgents.reduce((s, x) => s + x.avg_latency_ms, 0) / latAgents.length) : 0,
    }
  })()

  const options = [
    `<option value="fleet" ${forFleet ? 'selected' : ''}>Whole fleet</option>`,
    ...snap.agents.map(
      (a) => `<option value="${escapeHtml(a.id)}" ${model.selected === a.id ? 'selected' : ''}>${escapeHtml(a.name)}</option>`,
    ),
  ].join('')

  const buckets = bucketize(model.records, snap.now, windowSecs(model.window), bucketCount(model.window))
  const deliveries = model.loading
    ? `<div class="card-sub" style="padding:24px 0;text-align:center">Loading history…</div>`
    : deliveryChart(buckets)
  const latency = model.loading ? '' : latencyChart(buckets)

  // Activity: filter by agent, then paginate (10 per page, newest first).
  const filteredActivity = snap.activity.filter((e) => {
    if (model.activityAgent === 'all') return true
    if (model.activityAgent === 'system') return !e.agent_id
    return e.agent_id === model.activityAgent
  })
  const activityPages = Math.max(1, Math.ceil(filteredActivity.length / ACTIVITY_PER_PAGE))
  const activityPage = Math.min(Math.max(1, model.activityPage), activityPages)
  const pageSlice = filteredActivity.slice(
    (activityPage - 1) * ACTIVITY_PER_PAGE,
    activityPage * ACTIVITY_PER_PAGE,
  )
  const feed = pageSlice
    .map(
      (e) => `<div class="feed-item">
        <span class="feed-dot feed-${e.level}"></span>
        <span style="flex:1;min-width:0">${escapeHtml(e.text)}</span>
        <span class="feed-time">${compactTimestamp(e.ts)}</span>
      </div>`,
    )
    .join('')

  const activityFilterOptions = [
    `<option value="all" ${model.activityAgent === 'all' ? 'selected' : ''}>All agents</option>`,
    `<option value="system" ${model.activityAgent === 'system' ? 'selected' : ''}>System events</option>`,
    ...snap.agents.map(
      (a) =>
        `<option value="${escapeHtml(a.id)}" ${model.activityAgent === a.id ? 'selected' : ''}>${escapeHtml(a.name)}</option>`,
    ),
  ].join('')

  const activityPagination =
    activityPages > 1
      ? `<div class="pagination">
          <button class="btn btn-ghost btn-sm" data-action="activity-page" data-id="prev" ${activityPage === 1 ? 'disabled' : ''}>‹ Prev</button>
          <span class="page-label">Page ${activityPage} of ${activityPages}</span>
          <button class="btn btn-ghost btn-sm" data-action="activity-page" data-id="next" ${activityPage === activityPages ? 'disabled' : ''}>Next ›</button>
        </div>`
      : ''

  const activityEmpty =
    model.activityAgent === 'all'
      ? 'Nothing yet — activity shows up as the engine works.'
      : 'No activity for this selection yet.'

  return `<div class="content-inner">
    <div style="display:flex;align-items:center;justify-content:space-between;gap:12px;flex-wrap:wrap">
      <span class="section-label">Health & analytics</span>
      <div style="display:flex;gap:8px;align-items:center;flex-wrap:wrap">
        <select class="input" id="health-select" style="width:220px;flex-shrink:0">${options}</select>
        <div class="seg">
          <button data-action="health-window" data-id="24h" class="${model.window === '24h' ? 'active' : ''}">24h</button>
          <button data-action="health-window" data-id="7d" class="${model.window === '7d' ? 'active' : ''}">7 days</button>
        </div>
      </div>
    </div>

    <div class="kpi-grid">
      <div class="kpi">
        <div class="kpi-label">${icons.heart} ${agent ? 'Beats delivered' : 'Agents'}</div>
        <div class="kpi-value">${agent ? stats.total : snap.agents.length}</div>
        <div class="kpi-sub">${agent ? `${model.window} window` : `${inService} in service`}</div>
      </div>
      <div class="kpi">
        <div class="kpi-label">${icons.check} Delivery success</div>
        <div class="kpi-value">${stats.total > 0 ? `${stats.success_rate}%` : '—'}</div>
        <div class="kpi-sub">${stats.ok} of ${stats.total} beats reached Tragentics</div>
      </div>
      <div class="kpi">
        <div class="kpi-label">${icons.gauge} Avg beat latency</div>
        <div class="kpi-value">${stats.avg_latency_ms > 0 ? `${stats.avg_latency_ms}ms` : '—'}</div>
        <div class="kpi-sub">round-trip to the heartbeat API</div>
      </div>
      <div class="kpi">
        <div class="kpi-label">${icons.alert} Needs attention</div>
        <div class="kpi-value" style="${issues > 0 ? 'color:var(--status-offline)' : ''}">${issues}</div>
        <div class="kpi-sub">${issues === 0 ? 'all quiet' : 'halted, failing, or local-check issues'}</div>
      </div>
    </div>

    <div class="card">
      <div class="card-header" style="justify-content:space-between">
        <div><div class="card-title">Beat deliveries</div><div class="card-sub">${agent ? escapeHtml(agent.name) : 'All agents'} — each bar is a ${model.window === '24h' ? '30-minute' : '2-hour'} slice</div></div>
        <div class="legend"><span class="li"><span class="sw" style="background:#3b82f6"></span>delivered</span><span class="li"><span class="sw" style="background:#ef4444"></span>failed</span></div>
      </div>
      <div class="card-body chart-wrap" data-chart-host="delivery">${deliveries}</div>
    </div>

    <div class="card">
      <div class="card-header"><div><div class="card-title">Beat latency</div><div class="card-sub">average round-trip per slice</div></div></div>
      <div class="card-body chart-wrap" data-chart-host="latency">${latency}</div>
    </div>

    <div class="card">
      <div class="card-header"><div><div class="card-title">Delivery timeline</div><div class="card-sub">green = online beats · amber = honest offline (local check) · red = failures · gray = quiet</div></div></div>
      <div class="card-body">${model.loading ? '' : timelineStrip(model.records, snap.now, windowSecs(model.window))}</div>
    </div>

    <div class="card">
      <div class="card-header" style="justify-content:space-between">
        <div><div class="card-title">Activity</div><div class="card-sub">recent events from the engine${filteredActivity.length > 0 ? ` — ${filteredActivity.length} event${filteredActivity.length === 1 ? '' : 's'}` : ''}</div></div>
        <select class="input" id="activity-agent" style="width:200px;flex-shrink:0">${activityFilterOptions}</select>
      </div>
      <div class="feed" style="margin-top:10px">${feed || `<div class="empty" style="padding:22px"><p>${activityEmpty}</p></div>`}</div>
      ${activityPagination}
    </div>
  </div>`
}

/** Tooltip content for a delivery/latency bucket. */
export function bucketTooltip(bucket: Bucket, kind: 'delivery' | 'latency'): string {
  const time = compactTimestamp(bucket.start)
  if (kind === 'latency') {
    const avg = bucket.latencyN > 0 ? `${Math.round(bucket.latencySum / bucket.latencyN)}ms avg` : 'no samples'
    return `<div>${avg}</div><div class="tt-time">${time}</div>`
  }
  return `<div>${bucket.ok} delivered${bucket.fail > 0 ? ` · <span style="color:#f87171">${bucket.fail} failed</span>` : ''}</div><div class="tt-time">${time}</div>`
}
