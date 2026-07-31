// Bridge between the UI and the Rust core. In a real Tauri window it proxies
// invoke/listen; in a plain browser (design preview) it runs a deterministic
// demo simulation so the full UI is exercisable without any backend.

import type {
  AgentSettingsPatch,
  AgentView,
  BeatRecord,
  SettingsPatch,
  Snapshot,
  SparkPoint,
} from './types'

export interface Bridge {
  isDemo: boolean
  getSnapshot(): Promise<Snapshot>
  onSnapshot(cb: (s: Snapshot) => void): Promise<() => void>
  vaultInitialize(mode: 'keyring' | 'passphrase', passphrase?: string): Promise<void>
  vaultUnlock(passphrase: string): Promise<void>
  vaultLock(): Promise<void>
  vaultChangePassphrase(current: string, next: string): Promise<void>
  addAgent(token: string): Promise<string>
  removeAgent(agentId: string, sendOffline: boolean): Promise<void>
  setInService(agentId: string, inService: boolean): Promise<void>
  beatNow(agentId: string): Promise<void>
  updateAgent(agentId: string, patch: AgentSettingsPatch): Promise<void>
  refreshMe(agentId: string): Promise<void>
  getAgentHistory(agentId: string): Promise<BeatRecord[]>
  updateSettings(patch: SettingsPatch): Promise<void>
  setPaused(paused: boolean): Promise<void>
  setAutostart(enabled: boolean): Promise<void>
  testBaseUrl(url: string): Promise<string>
  openDataDir(): Promise<void>
  quitApp(): Promise<void>
}

const IS_TAURI = typeof window !== 'undefined' && '__TAURI_INTERNALS__' in window

// ── Tauri implementation ───────────────────────────────────────────────────

async function tauriBridge(): Promise<Bridge> {
  const { invoke } = await import('@tauri-apps/api/core')
  const { listen } = await import('@tauri-apps/api/event')
  return {
    isDemo: false,
    getSnapshot: () => invoke<Snapshot>('get_snapshot'),
    onSnapshot: async (cb) => {
      const un = await listen<Snapshot>('hcc://snapshot', (e) => cb(e.payload))
      return un
    },
    vaultInitialize: (mode, passphrase) => invoke('vault_initialize', { mode, passphrase: passphrase ?? null }),
    vaultUnlock: (passphrase) => invoke('vault_unlock', { passphrase }),
    vaultLock: () => invoke('vault_lock'),
    vaultChangePassphrase: (current, next) => invoke('vault_change_passphrase', { current, next }),
    addAgent: (token) => invoke<string>('add_agent', { token }),
    removeAgent: (agentId, sendOffline) => invoke('remove_agent', { agentId, sendOffline }),
    setInService: (agentId, inService) => invoke('set_in_service', { agentId, inService }),
    beatNow: (agentId) => invoke('beat_now', { agentId }),
    updateAgent: (agentId, patch) => invoke('update_agent', { agentId, patch }),
    refreshMe: (agentId) => invoke('refresh_me', { agentId }),
    getAgentHistory: (agentId) => invoke<BeatRecord[]>('get_agent_history', { agentId }),
    updateSettings: (patch) => invoke('update_settings', { patch }),
    setPaused: (paused) => invoke('set_paused', { paused }),
    setAutostart: (enabled) => invoke('set_autostart', { enabled }),
    testBaseUrl: (url) => invoke<string>('test_base_url', { url }),
    openDataDir: () => invoke('open_data_dir'),
    quitApp: () => invoke('quit_app'),
  }
}

// ── Demo implementation (browser preview only) ─────────────────────────────

const DEMO_NAMES = [
  'Invoice Parser',
  'Support Router',
  'Research Scout',
  'Ledger Sync',
  'Outreach Drafter',
  'Anomaly Watcher',
]

function demoSpark(seed: number, fail = 0): SparkPoint[] {
  const pts: SparkPoint[] = []
  for (let i = 0; i < 30; i++) {
    const bad = fail > 0 && i >= 30 - fail
    pts.push({ ok: !bad, latency_ms: bad ? null : 80 + Math.round(60 * Math.abs(Math.sin(seed + i / 3))) })
  }
  return pts
}

function demoBridge(): Bridge {
  const now = () => Math.floor(Date.now() / 1000)
  let idc = 0
  const uuid = () => `demo-${++idc}-${Math.random().toString(16).slice(2, 10)}`

  const mkAgent = (name: string, i: number): AgentView => ({
    id: uuid(),
    name,
    fingerprint: `tk_${(1234 + i).toString(16).padStart(4, '0')}…${(48879 + i).toString(16).slice(-4)}`,
    in_service: true,
    interval_secs: 300,
    local_check: i === 2 ? { url: 'http://127.0.0.1:8801/health', expect_min: 200, expect_max: 399, timeout_secs: 5 } : null,
    added_at: now() - 86400 * (i + 1),
    in_flight: false,
    consecutive_failures: 0,
    next_beat_at: now() + 30 + i * 47,
    last_attempt_at: now() - 60 * (i + 1),
    last_success_at: now() - 60 * (i + 1),
    last_kind: 'online_ok',
    last_note: null,
    last_latency_ms: 96 + i * 13,
    platform_status: 'online',
    platform_last_heartbeat: new Date(Date.now() - 60000 * (i + 1)).toISOString(),
    halted: null,
    local_check_failing: false,
    stats_24h: { total: 288, ok: 288 - i, success_rate: Math.round(((288 - i) / 288) * 100), avg_latency_ms: 104 + i * 9 },
    stats_7d: { total: 2016, ok: 2016 - i * 4, success_rate: 99, avg_latency_ms: 110 + i * 7 },
    spark: demoSpark(i),
  })

  const agents = DEMO_NAMES.map((n, i) => mkAgent(n, i))
  // Showcase states: one halted, one backing off, one honest-offline via local check.
  const halted = agents[4]
  if (halted) {
    halted.halted = { kind: 'token_invalid', message: 'Invalid API key' }
    halted.in_service = true
    halted.next_beat_at = null
    halted.last_kind = 'err_auth'
    halted.last_note = 'Invalid API key'
    halted.platform_status = 'offline'
    halted.spark = demoSpark(4, 6)
    halted.stats_24h = { total: 288, ok: 241, success_rate: 84, avg_latency_ms: 118 }
  }
  const degraded = agents[5]
  if (degraded) {
    degraded.consecutive_failures = 2
    degraded.last_kind = 'err_network'
    degraded.last_note = 'could not connect'
    degraded.next_beat_at = now() + 45
    degraded.spark = demoSpark(5, 2)
    degraded.stats_24h = { total: 288, ok: 279, success_rate: 97, avg_latency_ms: 131 }
  }
  const localFail = agents[2]
  if (localFail) {
    localFail.local_check_failing = true
    localFail.last_kind = 'local_fail_offline'
    localFail.last_note = 'local check failed: HTTP 503 outside expected 200–399'
    localFail.platform_status = 'offline'
  }
  const out = agents[3]
  if (out) {
    out.in_service = false
    out.next_beat_at = null
    out.last_kind = 'offline_ok'
    out.platform_status = 'offline'
  }

  // Seeded feed large enough to exercise pagination (10/page) + agent filtering.
  const activity: Snapshot['activity'] = [
    { ts: now() - 30, level: 'info', agent_id: agents[0]?.id ?? '', agent_name: 'Invoice Parser', text: 'Invoice Parser recovered — beating normally again' },
    { ts: now() - 140, level: 'error', agent_id: agents[4]?.id ?? '', agent_name: 'Outreach Drafter', text: 'Outreach Drafter halted: Invalid API key' },
    { ts: now() - 300, level: 'warn', agent_id: agents[5]?.id ?? '', agent_name: 'Anomaly Watcher', text: 'Anomaly Watcher: could not connect (retry in 60s, attempt 2)' },
    { ts: now() - 500, level: 'warn', agent_id: agents[2]?.id ?? '', agent_name: 'Research Scout', text: 'Research Scout: local check failed — reported offline to Tragentics' },
  ]
  for (let i = 0; i < 24; i++) {
    const agent = agents[i % agents.length]
    if (!agent) continue
    activity.push({
      ts: now() - 1200 - i * 700,
      level: i % 7 === 3 ? 'warn' : 'info',
      agent_id: agent.id,
      agent_name: agent.name,
      text:
        i % 7 === 3
          ? `${agent.name}: HTTP 500 Internal server error (retry in 30s, attempt 1)`
          : i % 5 === 2
            ? `${agent.name} put in service`
            : `${agent.name} recovered — beating normally again`,
    })
  }
  activity.push({ ts: now() - 22_000, level: 'info', text: 'Beating resumed' })
  activity.push({ ts: now() - 23_000, level: 'info', text: 'All beating paused' })
  activity.push({ ts: now() - 25_000, level: 'info', text: 'Local Vault created' })

  const snap: Snapshot = {
    version: '1.0.0 (demo)',
    vault_state: 'unlocked',
    vault_mode: 'keyring',
    paused: false,
    base_url: 'https://tragentics.com',
    theme: 'dark',
    minimize_to_tray: true,
    notify_on_problems: true,
    default_interval_secs: 300,
    autostart_enabled: true,
    tray: 'halted',
    now: now(),
    agents,
    activity,
  }

  let listener: ((s: Snapshot) => void) | null = null
  const push = () => {
    snap.now = now()
    listener?.(structuredClone(snap))
  }
  // Simulated beats: advance schedules, flip in_flight briefly.
  setInterval(() => {
    for (const a of snap.agents) {
      if (!a.in_service || a.halted || a.next_beat_at == null) continue
      if (a.next_beat_at <= now()) {
        a.in_flight = false
        a.last_attempt_at = now()
        a.last_success_at = now()
        a.last_kind = a.local_check_failing ? 'local_fail_offline' : 'online_ok'
        a.last_latency_ms = 80 + Math.round(Math.random() * 90)
        a.platform_status = a.local_check_failing ? 'offline' : 'online'
        a.platform_last_heartbeat = new Date().toISOString()
        a.next_beat_at = now() + 270 + Math.round(Math.random() * 60)
        a.spark = [...a.spark.slice(1), { ok: true, latency_ms: a.last_latency_ms }]
      }
    }
    push()
  }, 5000)

  const nofail = async () => {}
  return {
    isDemo: true,
    getSnapshot: async () => structuredClone(snap),
    onSnapshot: async (cb) => {
      listener = cb
      return () => {
        listener = null
      }
    },
    vaultInitialize: nofail,
    vaultUnlock: nofail,
    vaultLock: nofail,
    vaultChangePassphrase: nofail,
    addAgent: async (token) => {
      if (!/^tk_[0-9a-f]{64}$/.test(token.trim())) {
        throw "That doesn't look like an agent token — expected tk_ followed by 64 hex characters"
      }
      const a = mkAgent(`New Agent ${snap.agents.length + 1}`, snap.agents.length)
      a.platform_status = 'offline'
      a.next_beat_at = now() + 3
      snap.agents.push(a)
      snap.activity.unshift({ ts: now(), level: 'info', agent_name: a.name, text: `${a.name} added to the fleet — first beat on the way` })
      push()
      return a.id
    },
    removeAgent: async (id) => {
      snap.agents = snap.agents.filter((a) => a.id !== id)
      push()
    },
    setInService: async (id, inService) => {
      const a = snap.agents.find((x) => x.id === id)
      if (!a) return
      a.in_service = inService
      a.next_beat_at = inService ? now() + 2 : null
      if (!inService) {
        a.platform_status = 'offline'
        a.last_kind = 'offline_ok'
      }
      push()
    },
    beatNow: async (id) => {
      const a = snap.agents.find((x) => x.id === id)
      if (!a) return
      if (a.halted) a.halted = null
      a.next_beat_at = now()
      push()
    },
    updateAgent: async (id, patch) => {
      const a = snap.agents.find((x) => x.id === id)
      if (!a) return
      if (patch.interval_secs) a.interval_secs = patch.interval_secs
      if (patch.local_check !== undefined) a.local_check = patch.local_check
      push()
    },
    refreshMe: async () => push(),
    getAgentHistory: async (id) => {
      const a = snap.agents.find((x) => x.id === id)
      const records: BeatRecord[] = []
      const base = now()
      for (let i = 0; i < 288; i++) {
        const bad = a?.halted && i < 6
        records.unshift({
          ts: base - i * 300,
          kind: bad ? 'err_auth' : Math.random() < 0.015 ? 'err_network' : 'online_ok',
          http_status: bad ? 401 : 200,
          ...(bad ? {} : { latency_ms: 80 + Math.round(Math.random() * 120) }),
        })
      }
      return records
    },
    updateSettings: async (patch) => {
      Object.assign(snap, {
        ...(patch.base_url ? { base_url: patch.base_url } : {}),
        ...(patch.theme ? { theme: patch.theme } : {}),
        ...(patch.minimize_to_tray !== undefined ? { minimize_to_tray: patch.minimize_to_tray } : {}),
        ...(patch.notify_on_problems !== undefined ? { notify_on_problems: patch.notify_on_problems } : {}),
        ...(patch.default_interval_secs ? { default_interval_secs: patch.default_interval_secs } : {}),
      })
      push()
    },
    setPaused: async (paused) => {
      snap.paused = paused
      snap.tray = paused ? 'paused' : 'ok'
      push()
    },
    setAutostart: async (enabled) => {
      snap.autostart_enabled = enabled
      push()
    },
    testBaseUrl: async () => 'Reachable — Tragentics API answered as expected.',
    openDataDir: nofail,
    quitApp: nofail,
  }
}

let bridgePromise: Promise<Bridge> | null = null
export function getBridge(): Promise<Bridge> {
  if (!bridgePromise) {
    bridgePromise = IS_TAURI ? tauriBridge() : Promise.resolve(demoBridge())
  }
  return bridgePromise
}
