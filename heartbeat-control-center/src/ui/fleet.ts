// Fleet tab — line-by-line agent rows.

import { countdown, escapeHtml, intervalLabel, relativeTime, statusDotClass } from '../lib/format'
import { icons } from '../lib/icons'
import type { AgentView, Snapshot } from '../lib/types'
import { iconButton, sparkline, statusBadge, switchControl } from './components'

function beatSummary(agent: AgentView, now: number): string {
  if (agent.halted) {
    return `<div class="beat-line err-text">${icons.alert} ${escapeHtml(haltLabel(agent))}</div>
      <div class="beat-line">${escapeHtml(agent.halted.message)}</div>`
  }
  if (!agent.in_service) {
    return `<div class="beat-line">Out of service — reported offline</div>
      <div class="beat-line">Last beat ${relativeTime(agent.last_attempt_at, now)}</div>`
  }
  const lines: string[] = []
  if (agent.in_flight) {
    lines.push(`<div class="beat-line">${icons.activity} beating now…</div>`)
  } else if (agent.next_beat_at != null) {
    lines.push(
      `<div class="beat-line">last sent <span data-rel="${agent.last_success_at ?? ''}">${relativeTime(agent.last_success_at, now)}</span> · next in <span data-countdown="${agent.next_beat_at}">${countdown(agent.next_beat_at, now)}</span></div>`,
    )
  }
  if (agent.consecutive_failures > 0) {
    lines.push(
      `<div class="beat-line warn-text">${icons.alert} ${escapeHtml(agent.last_note ?? 'delivery failing')} (attempt ${agent.consecutive_failures})</div>`,
    )
  } else if (agent.local_check_failing) {
    lines.push(`<div class="beat-line warn-text">${icons.wifiOff} local check failing — reporting offline honestly</div>`)
  } else {
    lines.push(
      `<div class="beat-line">every ${intervalLabel(agent.interval_secs)}${agent.local_check ? ' · local check on' : ''}${agent.last_latency_ms != null ? ` · ${agent.last_latency_ms}ms` : ''}</div>`,
    )
  }
  return lines.slice(0, 2).join('')
}

export function haltLabel(agent: AgentView): string {
  switch (agent.halted?.kind) {
    case 'token_invalid':
      return 'Halted — token rejected'
    case 'token_mismatch':
      return 'Halted — token/agent mismatch'
    case 'revoked':
      return 'Halted — agent revoked'
    case 'archived':
      return 'Halted — agent archived'
    case 'auto_disabled':
      return 'Halted — agent auto-disabled'
    case 'not_found':
      return 'Halted — agent not found'
    case 'unavailable':
      return 'Halted — agent unavailable'
    default:
      return 'Halted'
  }
}

function agentRow(agent: AgentView, now: number): string {
  const dot = statusDotClass(agent.platform_status, agent.in_service, !!agent.halted)
  const pulse = agent.in_service && !agent.halted && agent.platform_status === 'online' ? ' dot-pulse' : ''
  return `<div class="agent-row ${agent.halted ? 'halted-row' : ''}" data-agent-row="${escapeHtml(agent.id)}">
    <span class="${dot}${pulse}"></span>
    <div class="agent-main row-click" data-action="open-drawer" data-id="${escapeHtml(agent.id)}" title="Open details">
      <div class="agent-name"><span class="name-text">${escapeHtml(agent.name)}</span> ${statusBadge(agent)}</div>
      <div class="agent-meta"><code>${escapeHtml(agent.fingerprint)}</code><span>·</span><span>24h ${agent.stats_24h.total > 0 ? `${agent.stats_24h.success_rate}%` : '—'}</span></div>
    </div>
    <div class="agent-beat">${beatSummary(agent, now)}</div>
    <div class="spark-cell" title="last 30 beats — blue ok, red failed">${sparkline(agent.spark)}</div>
    <div class="agent-actions">
      ${
        agent.halted
          ? `<button class="btn btn-sm btn-outline" data-action="retry-agent" data-id="${escapeHtml(agent.id)}">${icons.refresh} Retry</button>`
          : agent.in_service
            ? iconButton('beat-now', agent.id, 'bolt', 'Beat now')
            : ''
      }
      ${iconButton('open-drawer', agent.id, 'chevronRight', 'Details')}
      ${switchControl('toggle-service', agent.id, agent.in_service, agent.in_service ? 'In service — click to take out of service (reports offline)' : 'Out of service — click to resume beating')}
    </div>
  </div>`
}

export function renderFleet(snap: Snapshot, search: string): string {
  const q = search.trim().toLowerCase()
  const agents = q
    ? snap.agents.filter((a) => a.name.toLowerCase().includes(q) || a.fingerprint.toLowerCase().includes(q))
    : snap.agents

  if (snap.agents.length === 0) {
    return `<div class="content-inner">
      <div class="card"><div class="empty">
        <div class="empty-icon">${icons.heart}</div>
        <h3>No agents yet</h3>
        <p>Add your first agent and the Control Center starts beating for it immediately — it stays online on tragentics.com without you running a single script.</p>
        <div class="steps">
          <div class="step"><span class="n">1</span><span>Register an agent on tragentics.com — the token (<code>tk_…</code>) is shown once on the success screen.</span></div>
          <div class="step"><span class="n">2</span><span>Click <strong>Add Agent</strong> here and paste the token. That's the whole setup — the agent's identity is resolved automatically.</span></div>
          <div class="step"><span class="n">3</span><span>Leave the rest to the metronome: scheduled beats, honest offline reporting, backoff, and alerts when something needs you.</span></div>
        </div>
        <button class="btn btn-primary" data-action="open-add" style="margin-top:10px">${icons.plus} Add your first agent</button>
      </div></div>
    </div>`
  }

  const rows = agents.map((a) => agentRow(a, snap.now)).join('')
  const searchBox =
    snap.agents.length > 7
      ? `<input class="input" id="fleet-search" placeholder="Filter agents…" value="${escapeHtml(search)}" style="max-width:240px" />`
      : ''
  return `<div class="content-inner">
    <div style="display:flex;align-items:center;justify-content:space-between;gap:12px">
      <span class="section-label">Fleet — ${snap.agents.length} agent${snap.agents.length === 1 ? '' : 's'}, ${snap.agents.filter((a) => a.in_service).length} in service</span>
      ${searchBox}
    </div>
    <div class="card"><div class="agent-list">${rows || `<div class="empty"><p>No agents match “${escapeHtml(search)}”.</p></div>`}</div></div>
    ${
      snap.paused
        ? `<div class="form-error">${icons.pause} All beating is paused — agents will drift idle on the platform after 15 minutes and offline after that. Resume from the button above or the tray.</div>`
        : ''
    }
  </div>`
}
