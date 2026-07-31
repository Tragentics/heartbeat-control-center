import './styles.css'
import { getBridge, type Bridge } from './lib/bridge'
import { countdown, escapeHtml, relativeTime } from './lib/format'
import { icons } from './lib/icons'
import type { BeatRecord, Snapshot } from './lib/types'
import { bucketize, type Bucket } from './ui/components'
import { renderFleet } from './ui/fleet'
import { renderGate } from './ui/gate'
import { bucketCount, bucketTooltip, renderHealth, windowSecs, type HealthModel, type HealthWindow } from './ui/health'
import { renderAddModal, renderDrawer, renderPassphraseModal, renderRemoveModal } from './ui/modals'
import { renderSettings } from './ui/settings'

type Tab = 'fleet' | 'health' | 'settings'
type Modal = 'add' | 'remove' | 'passphrase' | null

interface UiState {
  snap: Snapshot | null
  tab: Tab
  modal: Modal
  modalAgentId: string | null
  drawerAgentId: string | null
  search: string
  gateMode: 'keyring' | 'passphrase'
  health: HealthModel
  busy: boolean
}

const state: UiState = {
  snap: null,
  tab: 'fleet',
  modal: null,
  modalAgentId: null,
  drawerAgentId: null,
  search: '',
  gateMode: 'keyring',
  health: {
    selected: 'fleet',
    window: '24h',
    records: [],
    loading: false,
    activityPage: 1,
    activityAgent: 'all',
  },
  busy: false,
}

let bridge: Bridge
const app = document.getElementById('app')!
const historyCache = new Map<string, BeatRecord[]>()
let healthBuckets: Bucket[] = []

// ── Theme ──────────────────────────────────────────────────────────────────

const media = window.matchMedia('(prefers-color-scheme: dark)')
function applyTheme(): void {
  const theme = state.snap?.theme ?? 'system'
  const dark = theme === 'dark' || (theme === 'system' && media.matches)
  document.documentElement.classList.toggle('dark', dark)
}
media.addEventListener('change', applyTheme)

// ── Toasts ─────────────────────────────────────────────────────────────────

const toastHost = document.createElement('div')
toastHost.className = 'toasts'
document.body.appendChild(toastHost)

function toast(message: string, kind: 'ok' | 'err' | 'info' = 'info'): void {
  const el = document.createElement('div')
  el.className = `toast ${kind === 'info' ? '' : kind}`
  el.innerHTML = `${kind === 'err' ? icons.alert : kind === 'ok' ? icons.check : icons.info}<span>${escapeHtml(message)}</span>`
  toastHost.appendChild(el)
  setTimeout(() => {
    el.style.transition = 'opacity 200ms ease'
    el.style.opacity = '0'
    setTimeout(() => el.remove(), 220)
  }, 4200)
}

function errText(e: unknown): string {
  if (typeof e === 'string') return e
  if (e instanceof Error) return e.message
  return String(e)
}

// ── Rendering ──────────────────────────────────────────────────────────────

function render(): void {
  const snap = state.snap
  if (!snap) {
    app.innerHTML = `<div class="gate"><div class="card-sub">Starting…</div></div>`
    return
  }
  applyTheme()

  if (snap.vault_state !== 'unlocked') {
    app.innerHTML = renderHeader(snap, true) + renderGate(snap, state.gateMode)
    if (snap.vault_state === 'locked') {
      ;(document.getElementById('unlock-pass') as HTMLInputElement | null)?.focus()
    }
    return
  }

  const tabHtml =
    state.tab === 'fleet'
      ? renderFleet(snap, state.search)
      : state.tab === 'health'
        ? renderHealth(snap, state.health)
        : renderSettings(snap)

  const drawerAgent = state.drawerAgentId ? snap.agents.find((a) => a.id === state.drawerAgentId) : null
  const modalHtml =
    state.modal === 'add'
      ? renderAddModal()
      : state.modal === 'remove' && state.modalAgentId
        ? (() => {
            const agent = snap.agents.find((a) => a.id === state.modalAgentId)
            return agent ? renderRemoveModal(agent) : ''
          })()
        : state.modal === 'passphrase'
          ? renderPassphraseModal()
          : ''

  app.innerHTML =
    renderHeader(snap, false) +
    `<nav class="tabs">
      <button class="tab ${state.tab === 'fleet' ? 'active' : ''}" data-action="tab" data-id="fleet">${icons.heart} Fleet</button>
      <button class="tab ${state.tab === 'health' ? 'active' : ''}" data-action="tab" data-id="health">${icons.activity} Health</button>
      <button class="tab ${state.tab === 'settings' ? 'active' : ''}" data-action="tab" data-id="settings">${icons.settings} Settings</button>
    </nav>
    <main class="content">${tabHtml}</main>` +
    (drawerAgent ? renderDrawer(snap, drawerAgent) : '') +
    modalHtml

  if (state.modal === 'add') {
    const input = document.getElementById('add-token') as HTMLInputElement | null
    input?.focus()
  }
}

function renderHeader(snap: Snapshot, gate: boolean): string {
  const inService = snap.agents.filter((a) => a.in_service).length
  const issues = snap.agents.filter((a) => a.halted || a.consecutive_failures > 0 || a.local_check_failing).length
  const pills = gate
    ? ''
    : `<div class="titlebar-status">
        <span class="pill"><span class="dot ${snap.paused ? 'dot-off' : 'dot-ok'}"></span>${snap.paused ? 'paused' : `${inService} beating`}</span>
        ${issues > 0 ? `<span class="pill pill-danger">${icons.alert} ${issues} need${issues === 1 ? 's' : ''} attention</span>` : ''}
        ${bridge?.isDemo ? `<span class="pill pill-warn">design preview — simulated data</span>` : ''}
      </div>`
  const actions = gate
    ? ''
    : `<div class="titlebar-actions">
        <button class="btn btn-ghost btn-sm" data-action="toggle-pause" title="${snap.paused ? 'Resume beating' : 'Pause all beating'}">
          ${snap.paused ? `${icons.play} Resume` : `${icons.pause} Pause`}
        </button>
        <button class="btn btn-primary btn-sm" data-action="open-add">${icons.plus} Add Agent</button>
      </div>`
  return `<header class="titlebar">
    <div class="brand">
      <div class="brand-mark">${icons.heart}</div>
      <div class="brand-text">
        <div class="wordmark">Tragentics</div>
        <div class="app-name">Heartbeat Control Center</div>
      </div>
    </div>
    ${pills}${actions}
  </header>`
}

// ── Live text updates (no re-render) ───────────────────────────────────────

setInterval(() => {
  if (!state.snap) return
  const now = Math.floor(Date.now() / 1000)
  document.querySelectorAll<HTMLElement>('[data-countdown]').forEach((el) => {
    const ts = Number(el.dataset.countdown)
    el.textContent = countdown(Number.isFinite(ts) ? ts : null, now)
  })
  document.querySelectorAll<HTMLElement>('[data-rel]').forEach((el) => {
    const raw = el.dataset.rel
    el.textContent = raw ? relativeTime(Number(raw), now) : '—'
  })
}, 1000)

// ── Health data ────────────────────────────────────────────────────────────

async function loadHealth(): Promise<void> {
  const snap = state.snap
  if (!snap) return
  state.health.loading = true
  render()
  try {
    const ids = state.health.selected === 'fleet' ? snap.agents.map((a) => a.id) : [state.health.selected]
    const all: BeatRecord[] = []
    for (const id of ids) {
      let records = historyCache.get(id)
      if (!records) {
        records = await bridge.getAgentHistory(id)
        historyCache.set(id, records)
      }
      all.push(...records)
    }
    all.sort((a, b) => a.ts - b.ts)
    state.health.records = all
  } catch (e) {
    toast(errText(e), 'err')
    state.health.records = []
  }
  state.health.loading = false
  if (state.snap) {
    healthBuckets = bucketize(
      state.health.records,
      state.snap.now,
      windowSecs(state.health.window),
      bucketCount(state.health.window),
    )
  }
  render()
}

// ── Chart tooltips ─────────────────────────────────────────────────────────

let tooltipEl: HTMLElement | null = null
function showTooltip(x: number, y: number, html: string): void {
  if (!tooltipEl) {
    tooltipEl = document.createElement('div')
    tooltipEl.className = 'chart-tooltip'
    document.body.appendChild(tooltipEl)
  }
  tooltipEl.innerHTML = html
  tooltipEl.style.left = `${x}px`
  tooltipEl.style.top = `${y}px`
  tooltipEl.style.display = 'block'
}
function hideTooltip(): void {
  if (tooltipEl) tooltipEl.style.display = 'none'
}

document.addEventListener('mousemove', (e) => {
  const target = e.target as HTMLElement
  const rect = target.closest?.('[data-bi]') as SVGElement | null
  if (rect && rect.closest('[data-chart]')) {
    const chart = (rect.closest('[data-chart]') as SVGElement).dataset.chart as 'delivery' | 'latency'
    const idx = Number((rect as unknown as HTMLElement).dataset.bi)
    const bucket = healthBuckets[idx]
    if (bucket) {
      showTooltip(e.clientX, e.clientY, bucketTooltip(bucket, chart))
      return
    }
  }
  hideTooltip()
})

// ── Actions ────────────────────────────────────────────────────────────────

function setGateError(message: string): void {
  const host = document.getElementById('gate-error')
  if (host) host.innerHTML = message ? `<div class="form-error">${icons.alert}<span>${escapeHtml(message)}</span></div>` : ''
}

async function guard<T>(fn: () => Promise<T>): Promise<T | undefined> {
  if (state.busy) return undefined
  state.busy = true
  try {
    return await fn()
  } catch (e) {
    toast(errText(e), 'err')
    return undefined
  } finally {
    state.busy = false
  }
}

async function handleAction(action: string, id: string, target: HTMLElement): Promise<void> {
  switch (action) {
    case 'tab': {
      state.tab = id as Tab
      state.search = ''
      render()
      if (state.tab === 'health') void loadHealth()
      break
    }
    case 'activity-page': {
      state.health.activityPage = id === 'prev' ? Math.max(1, state.health.activityPage - 1) : state.health.activityPage + 1
      render()
      break
    }
    case 'open-add': {
      state.modal = 'add'
      render()
      break
    }
    case 'close-modal': {
      state.modal = null
      state.modalAgentId = null
      render()
      break
    }
    case 'confirm-add': {
      const input = document.getElementById('add-token') as HTMLInputElement | null
      const errHost = document.getElementById('add-error')
      const btn = document.getElementById('confirm-add-btn') as HTMLButtonElement | null
      const token = input?.value.trim() ?? ''
      if (!/^tk_[0-9a-f]{64}$/.test(token)) {
        if (errHost)
          errHost.innerHTML = `<div class="form-error">${icons.alert}<span>That doesn't look like an agent token — expected <code>tk_</code> followed by 64 hex characters.</span></div>`
        return
      }
      if (btn) {
        btn.disabled = true
        btn.textContent = 'Verifying with Tragentics…'
      }
      try {
        const agentId = await bridge.addAgent(token)
        state.modal = null
        historyCache.delete(agentId)
        await refreshSnapshot()
        const agent = state.snap?.agents.find((a) => a.id === agentId)
        toast(`${agent?.name ?? 'Agent'} added — first beat is on the way`, 'ok')
      } catch (e) {
        // Add failed — restore the button and surface the error inline.
        if (errHost)
          errHost.innerHTML = `<div class="form-error">${icons.alert}<span>${escapeHtml(errText(e))}</span></div>`
        if (btn) {
          btn.disabled = false
          btn.innerHTML = `${icons.shield} Verify &amp; add`
        }
      }
      break
    }
    case 'open-remove': {
      state.modal = 'remove'
      state.modalAgentId = id
      render()
      break
    }
    case 'confirm-remove': {
      const sendOffline = (document.getElementById('remove-offline') as HTMLInputElement | null)?.checked ?? true
      await guard(async () => {
        await bridge.removeAgent(id, sendOffline)
        state.modal = null
        state.modalAgentId = null
        if (state.drawerAgentId === id) state.drawerAgentId = null
        historyCache.delete(id)
        await refreshSnapshot()
        toast('Agent removed', 'ok')
      })
      break
    }
    case 'open-drawer': {
      state.drawerAgentId = id
      render()
      break
    }
    case 'close-drawer': {
      state.drawerAgentId = null
      render()
      break
    }
    case 'beat-now': {
      await guard(() => bridge.beatNow(id))
      break
    }
    case 'retry-agent': {
      await guard(() => bridge.beatNow(id))
      toast('Retrying — halt cleared', 'info')
      break
    }
    case 'refresh-me': {
      await guard(async () => {
        await bridge.refreshMe(id)
        toast('Platform state refreshed', 'ok')
      })
      break
    }
    case 'toggle-pause': {
      const paused = state.snap?.paused ?? false
      await guard(() => bridge.setPaused(!paused))
      break
    }
    case 'toggle-local-check': {
      const enabled = (target as HTMLInputElement).checked
      await guard(() =>
        bridge.updateAgent(id, {
          local_check: enabled
            ? { url: 'http://127.0.0.1:8080/health', expect_min: 200, expect_max: 399, timeout_secs: 5 }
            : null,
        }),
      )
      render()
      break
    }
    case 'save-local-check': {
      const url = (document.getElementById('lc-url') as HTMLInputElement | null)?.value.trim() ?? ''
      const min = Number((document.getElementById('lc-min') as HTMLInputElement | null)?.value ?? 200)
      const max = Number((document.getElementById('lc-max') as HTMLInputElement | null)?.value ?? 399)
      const timeout = Number((document.getElementById('lc-timeout') as HTMLInputElement | null)?.value ?? 5)
      const errHost = document.getElementById('lc-error')
      if (!/^https?:\/\//i.test(url)) {
        if (errHost)
          errHost.innerHTML = `<div class="form-error" style="margin-top:8px">${icons.alert}<span>Health URL must start with http:// or https://</span></div>`
        return
      }
      await guard(async () => {
        await bridge.updateAgent(id, {
          local_check: { url, expect_min: min, expect_max: max, timeout_secs: timeout },
        })
        toast('Local check saved', 'ok')
      })
      break
    }
    case 'open-passphrase': {
      state.modal = 'passphrase'
      render()
      break
    }
    case 'confirm-passphrase': {
      const current = (document.getElementById('pp-current') as HTMLInputElement | null)?.value ?? ''
      const next = (document.getElementById('pp-next') as HTMLInputElement | null)?.value ?? ''
      const next2 = (document.getElementById('pp-next2') as HTMLInputElement | null)?.value ?? ''
      const errHost = document.getElementById('pp-error')
      if (next !== next2) {
        if (errHost) errHost.innerHTML = `<div class="form-error">${icons.alert}<span>New passphrases don't match.</span></div>`
        return
      }
      await guard(async () => {
        await bridge.vaultChangePassphrase(current, next)
        state.modal = null
        render()
        toast('Passphrase changed', 'ok')
      })
      break
    }
    case 'lock-vault': {
      await guard(() => bridge.vaultLock())
      break
    }
    case 'toggle-autostart': {
      const checked = (target as HTMLInputElement).checked
      await guard(() => bridge.setAutostart(checked))
      break
    }
    case 'toggle-minimize': {
      await guard(() => bridge.updateSettings({ minimize_to_tray: (target as HTMLInputElement).checked }))
      break
    }
    case 'toggle-notify': {
      await guard(() => bridge.updateSettings({ notify_on_problems: (target as HTMLInputElement).checked }))
      break
    }
    case 'set-theme': {
      await guard(() => bridge.updateSettings({ theme: id }))
      break
    }
    case 'toggle-service': {
      const checked = (target as HTMLInputElement).checked
      await guard(() => bridge.setInService(id, checked))
      break
    }
    case 'test-base-url': {
      const url = (document.getElementById('base-url') as HTMLInputElement | null)?.value ?? ''
      const host = document.getElementById('base-url-result')
      if (host) host.innerHTML = `<div class="hint" style="margin-top:6px">Testing…</div>`
      try {
        const message = await bridge.testBaseUrl(url)
        if (host) host.innerHTML = `<div class="form-ok" style="margin-top:6px">${icons.check}<span>${escapeHtml(message)}</span></div>`
      } catch (e) {
        if (host) host.innerHTML = `<div class="form-error" style="margin-top:6px">${icons.alert}<span>${escapeHtml(errText(e))}</span></div>`
      }
      break
    }
    case 'save-base-url': {
      const url = (document.getElementById('base-url') as HTMLInputElement | null)?.value ?? ''
      await guard(async () => {
        await bridge.updateSettings({ base_url: url })
        toast('Base URL saved', 'ok')
      })
      break
    }
    case 'open-data-dir': {
      await guard(() => bridge.openDataDir())
      break
    }
    case 'open-site': {
      window.open('https://tragentics.com', '_blank')
      break
    }
    case 'quit-app': {
      await guard(() => bridge.quitApp())
      break
    }
    case 'pick-mode': {
      state.gateMode = id as 'keyring' | 'passphrase'
      render()
      break
    }
    case 'create-vault': {
      if (state.gateMode === 'passphrase') {
        const p1 = (document.getElementById('setup-pass') as HTMLInputElement | null)?.value ?? ''
        const p2 = (document.getElementById('setup-pass2') as HTMLInputElement | null)?.value ?? ''
        if (p1.length < 10) return setGateError('Passphrase must be at least 10 characters.')
        if (p1 !== p2) return setGateError("Passphrases don't match.")
        await guard(async () => {
          await bridge.vaultInitialize('passphrase', p1)
          await refreshSnapshot()
        })
      } else {
        await guard(async () => {
          await bridge.vaultInitialize('keyring')
          await refreshSnapshot()
        })
      }
      break
    }
    case 'unlock-vault': {
      const pass = (document.getElementById('unlock-pass') as HTMLInputElement | null)?.value ?? ''
      try {
        await bridge.vaultUnlock(pass)
        await refreshSnapshot()
      } catch (e) {
        setGateError(errText(e))
      }
      break
    }
    case 'retry-vault': {
      await refreshSnapshot()
      break
    }
    default:
      break
  }
}

// Event delegation.
document.addEventListener('click', (e) => {
  const target = (e.target as HTMLElement).closest?.('[data-action]') as HTMLElement | null
  if (!target) return
  // Switches handle their action on 'change', not click.
  if (target instanceof HTMLInputElement && target.type === 'checkbox') return
  const action = target.dataset.action!
  const id = target.dataset.id ?? ''
  void handleAction(action, id, target)
})

document.addEventListener('change', (e) => {
  const target = e.target as HTMLElement
  if (target instanceof HTMLInputElement && target.dataset.action) {
    void handleAction(target.dataset.action, target.dataset.id ?? '', target)
    return
  }
  if (target instanceof HTMLSelectElement) {
    if (target.id === 'health-select') {
      state.health.selected = target.value
      void loadHealth()
    } else if (target.id === 'activity-agent') {
      state.health.activityAgent = target.value
      state.health.activityPage = 1
      render()
    } else if (target.id === 'default-interval') {
      void guard(() => bridge.updateSettings({ default_interval_secs: Number(target.value) }))
    } else if (target.dataset.actionChange === 'set-interval' && target.dataset.id) {
      void guard(() => bridge.updateAgent(target.dataset.id!, { interval_secs: Number(target.value) }))
    }
  }
})

document.addEventListener('input', (e) => {
  const target = e.target as HTMLElement
  if (target instanceof HTMLInputElement && target.id === 'fleet-search') {
    state.search = target.value
    // Re-render only the list body to keep input focus.
    const snap = state.snap
    if (!snap) return
    const content = document.querySelector('.content')
    if (content) {
      const scroll = content.scrollTop
      content.innerHTML = renderFleet(snap, state.search)
      content.scrollTop = scroll
      const input = document.getElementById('fleet-search') as HTMLInputElement | null
      if (input) {
        input.focus()
        input.setSelectionRange(input.value.length, input.value.length)
      }
    }
  }
})

document.addEventListener('keydown', (e) => {
  if (e.key === 'Escape') {
    if (state.modal) {
      state.modal = null
      state.modalAgentId = null
      render()
    } else if (state.drawerAgentId) {
      state.drawerAgentId = null
      render()
    }
  }
  if ((e.ctrlKey || e.metaKey) && e.key.toLowerCase() === 'n' && state.snap?.vault_state === 'unlocked') {
    e.preventDefault()
    state.modal = 'add'
    render()
  }
})

async function refreshSnapshot(): Promise<void> {
  state.snap = await bridge.getSnapshot()
  render()
}

// ── Boot ───────────────────────────────────────────────────────────────────

async function boot(): Promise<void> {
  bridge = await getBridge()
  await bridge.onSnapshot((s) => {
    const vaultWasLocked = state.snap?.vault_state !== 'unlocked'
    state.snap = s
    // History changed server-side — nuke cache lazily (cheap; local IPC).
    historyCache.clear()
    // Preserve typing contexts: skip re-render while a modal is open or any
    // input has focus — EXCEPT when the vault state flips (gates must update).
    const vaultFlipped = vaultWasLocked !== (s.vault_state !== 'unlocked')
    const active = document.activeElement
    const typing =
      active instanceof HTMLInputElement ||
      active instanceof HTMLTextAreaElement ||
      active instanceof HTMLSelectElement
    if ((state.modal === null && !typing) || vaultFlipped) {
      render()
      if (state.tab === 'health') void loadHealth()
    }
  })
  await refreshSnapshot()
  if (state.tab === 'health') void loadHealth()
}

// Health-window buttons need the action map too.
document.addEventListener('click', (e) => {
  const target = (e.target as HTMLElement).closest?.('[data-action="health-window"]') as HTMLElement | null
  if (!target) return
  state.health.window = (target.dataset.id ?? '24h') as HealthWindow
  void loadHealth()
})

void boot()
