// Modals + the agent detail drawer.

import { compactTimestamp, escapeHtml, intervalLabel, relativeIso, relativeTime } from '../lib/format'
import { icons } from '../lib/icons'
import type { AgentView, Snapshot } from '../lib/types'
import { sparkline, statusBadge } from './components'
import { haltLabel } from './fleet'

export function renderAddModal(): string {
  return `<div class="overlay" data-overlay="add">
    <div class="modal" role="dialog" aria-modal="true" aria-label="Add agent">
      <div class="modal-header">
        ${icons.plus}<span class="modal-title">Add an agent</span>
        <button class="btn-icon" style="margin-left:auto" data-action="close-modal" aria-label="Close">${icons.x}</button>
      </div>
      <div class="modal-body">
        <p style="font-size:12.5px;color:var(--muted-foreground)">Paste the agent token from the registration screen on tragentics.com — the one shown once, starting with <code>tk_</code>. The agent's identity is resolved from the token automatically and the token goes straight into the encrypted vault.</p>
        <div class="field">
          <label for="add-token">Agent token</label>
          <input id="add-token" class="input mono" type="password" placeholder="tk_…" spellcheck="false" autocomplete="off" />
          <span class="hint" id="add-token-hint">Expected format: tk_ followed by 64 hex characters.</span>
        </div>
        <div id="add-error"></div>
      </div>
      <div class="modal-footer">
        <button class="btn btn-ghost" data-action="close-modal">Cancel</button>
        <button class="btn btn-primary" data-action="confirm-add" id="confirm-add-btn">${icons.shield} Verify &amp; add</button>
      </div>
    </div>
  </div>`
}

export function renderRemoveModal(agent: AgentView): string {
  return `<div class="overlay" data-overlay="remove">
    <div class="modal" role="dialog" aria-modal="true" aria-label="Remove agent">
      <div class="modal-header">
        ${icons.trash}<span class="modal-title">Remove ${escapeHtml(agent.name)}?</span>
        <button class="btn-icon" style="margin-left:auto" data-action="close-modal" aria-label="Close">${icons.x}</button>
      </div>
      <div class="modal-body">
        <p style="font-size:12.5px;color:var(--muted-foreground)">This deletes the token from the local vault. The agent itself is untouched on tragentics.com — but without beats it will drift idle in 15 minutes, and its token cannot be re-shown there, so make sure it's saved somewhere else if you'll need it again.</p>
        <label style="display:flex;align-items:center;gap:9px;font-size:13px;cursor:pointer">
          <input type="checkbox" id="remove-offline" checked style="width:15px;height:15px;accent-color:var(--chart-blue);cursor:pointer" />
          Report the agent offline first (recommended — honest handoff)
        </label>
      </div>
      <div class="modal-footer">
        <button class="btn btn-ghost" data-action="close-modal">Cancel</button>
        <button class="btn btn-danger" data-action="confirm-remove" data-id="${escapeHtml(agent.id)}">${icons.trash} Remove agent</button>
      </div>
    </div>
  </div>`
}

export function renderPassphraseModal(): string {
  return `<div class="overlay" data-overlay="passphrase">
    <div class="modal" role="dialog" aria-modal="true" aria-label="Change passphrase">
      <div class="modal-header">
        ${icons.key}<span class="modal-title">Change vault passphrase</span>
        <button class="btn-icon" style="margin-left:auto" data-action="close-modal" aria-label="Close">${icons.x}</button>
      </div>
      <div class="modal-body">
        <div class="field"><label for="pp-current">Current passphrase</label><input id="pp-current" class="input" type="password" autocomplete="current-password" /></div>
        <div class="field"><label for="pp-next">New passphrase <span class="hint">(minimum 10 characters)</span></label><input id="pp-next" class="input" type="password" autocomplete="new-password" /></div>
        <div class="field"><label for="pp-next2">Confirm new passphrase</label><input id="pp-next2" class="input" type="password" autocomplete="new-password" /></div>
        <div id="pp-error"></div>
      </div>
      <div class="modal-footer">
        <button class="btn btn-ghost" data-action="close-modal">Cancel</button>
        <button class="btn btn-primary" data-action="confirm-passphrase">${icons.key} Change passphrase</button>
      </div>
    </div>
  </div>`
}

export function renderDrawer(snap: Snapshot, agent: AgentView): string {
  const me = agent.platform_status
  const lc = agent.local_check
  return `<div class="drawer-overlay" data-action="close-drawer"></div>
  <div class="drawer" role="dialog" aria-label="${escapeHtml(agent.name)} details">
    <div class="drawer-header">
      <div style="min-width:0">
        <div class="agent-name" style="font-size:15px"><span class="name-text">${escapeHtml(agent.name)}</span> ${statusBadge(agent)}</div>
        <div class="agent-meta"><code>${escapeHtml(agent.fingerprint)}</code></div>
      </div>
      <button class="btn-icon" style="margin-left:auto" data-action="close-drawer" aria-label="Close">${icons.x}</button>
    </div>
    <div class="drawer-body">
      ${
        agent.halted
          ? `<div class="form-error">${icons.alert}<span><strong>${escapeHtml(haltLabel(agent))}</strong><br/>${escapeHtml(agent.halted.message)}${
              agent.halted.kind === 'token_invalid' || agent.halted.kind === 'token_mismatch'
                ? '<br/>The stored token no longer works. Remove this agent and re-add it with a valid token.'
                : agent.halted.kind === 'auto_disabled'
                  ? '<br/>Resolve the account issue on tragentics.com, then retry.'
                  : ''
            }</span></div>
            <button class="btn btn-outline" data-action="retry-agent" data-id="${escapeHtml(agent.id)}">${icons.refresh} Clear halt &amp; retry</button>`
          : ''
      }

      <div>
        <span class="section-label">Platform truth</span>
        <dl class="kv" style="margin-top:8px">
          <dt>Status on Tragentics</dt><dd>${me ? escapeHtml(me) : '—'}</dd>
          <dt>Platform last heartbeat</dt><dd>${agent.platform_last_heartbeat ? `${relativeIso(agent.platform_last_heartbeat, snap.now)} <span style="color:var(--muted-foreground)">(${compactTimestamp(agent.platform_last_heartbeat)})</span>` : '—'}</dd>
          <dt>Agent ID</dt><dd><code style="font-size:11px">${escapeHtml(agent.id)}</code></dd>
          <dt>Added</dt><dd>${compactTimestamp(agent.added_at)}</dd>
        </dl>
        <button class="btn btn-ghost btn-sm" data-action="refresh-me" data-id="${escapeHtml(agent.id)}" style="margin-top:8px">${icons.refresh} Re-check with Tragentics</button>
      </div>

      <div>
        <span class="section-label">Beating</span>
        <dl class="kv" style="margin-top:8px">
          <dt>Last beat sent</dt><dd><span data-rel="${agent.last_success_at ?? ''}">${relativeTime(agent.last_success_at, snap.now)}</span>${agent.last_latency_ms != null ? ` · ${agent.last_latency_ms}ms` : ''}</dd>
          <dt>Delivery (24h)</dt><dd>${agent.stats_24h.total > 0 ? `${agent.stats_24h.success_rate}% of ${agent.stats_24h.total}` : '—'}</dd>
          <dt>Delivery (7d)</dt><dd>${agent.stats_7d.total > 0 ? `${agent.stats_7d.success_rate}% of ${agent.stats_7d.total}` : '—'}</dd>
        </dl>
        <div style="margin-top:8px">${sparkline(agent.spark, 200, 34)}</div>
        <div class="field" style="margin-top:12px">
          <label for="drawer-interval">Beat interval</label>
          <select id="drawer-interval" class="input" data-action-change="set-interval" data-id="${escapeHtml(agent.id)}" style="max-width:160px">
            ${[300, 480, 600, 840]
              .map((s) => `<option value="${s}" ${agent.interval_secs === s ? 'selected' : ''}>${intervalLabel(s)}</option>`)
              .join('')}
          </select>
          <span class="hint">5 minutes is the floor — use Beat now for an on-demand beat. Tragentics marks agents idle after 15 quiet minutes, so every option keeps margin.</span>
        </div>
        <div style="display:flex;gap:8px;margin-top:10px">
          <button class="btn btn-outline btn-sm" data-action="beat-now" data-id="${escapeHtml(agent.id)}">${icons.bolt} Beat now</button>
        </div>
      </div>

      <div>
        <span class="section-label">Local health check</span>
        <p class="hint" style="margin-top:6px">For self-hosted agents: the Control Center probes <em>your</em> server before each beat and reports <strong>offline honestly</strong> when it fails — instead of asserting a liveness that isn't true. Leave off for agents backed by third-party APIs.</p>
        <div style="display:flex;align-items:center;gap:10px;margin-top:10px">
          <label class="switch" title="Enable local check">
            <input type="checkbox" id="lc-enabled" data-action="toggle-local-check" data-id="${escapeHtml(agent.id)}" ${lc ? 'checked' : ''} />
            <span class="track"></span><span class="thumb"></span>
          </label>
          <span style="font-size:13px">${lc ? 'Enabled — beats verify your endpoint first' : 'Disabled — beats assert in-service status'}</span>
        </div>
        ${
          lc
            ? `<div class="field" style="margin-top:10px">
                <label for="lc-url">Health URL</label>
                <input id="lc-url" class="input mono" value="${escapeHtml(lc.url)}" spellcheck="false" placeholder="http://127.0.0.1:8080/health" />
              </div>
              <div style="display:flex;gap:10px;margin-top:8px">
                <div class="field" style="flex:1"><label for="lc-min">Accept from</label><input id="lc-min" class="input" type="number" min="100" max="599" value="${lc.expect_min}" /></div>
                <div class="field" style="flex:1"><label for="lc-max">to (status)</label><input id="lc-max" class="input" type="number" min="100" max="599" value="${lc.expect_max}" /></div>
                <div class="field" style="flex:1"><label for="lc-timeout">Timeout (s)</label><input id="lc-timeout" class="input" type="number" min="1" max="30" value="${lc.timeout_secs}" /></div>
              </div>
              <div id="lc-error"></div>
              <button class="btn btn-outline btn-sm" data-action="save-local-check" data-id="${escapeHtml(agent.id)}" style="margin-top:8px">${icons.check} Save check</button>`
            : ''
        }
      </div>

      <div>
        <span class="section-label">Danger zone</span>
        <div style="display:flex;gap:8px;margin-top:8px">
          <button class="btn btn-danger btn-sm" data-action="open-remove" data-id="${escapeHtml(agent.id)}">${icons.trash} Remove from Control Center</button>
        </div>
      </div>
    </div>
  </div>`
}
