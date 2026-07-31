// View-model types — mirror the Rust serde output exactly (snake_case enums).

export type VaultState = 'uninitialized' | 'locked' | 'unlocked' | 'keyring_unavailable'
export type VaultMode = 'keyring' | 'passphrase'
export type TraySeverity = 'ok' | 'degraded' | 'halted' | 'paused'

export type BeatKind =
  | 'online_ok'
  | 'offline_ok'
  | 'local_fail_offline'
  | 'err_network'
  | 'err_auth'
  | 'err_rate_limited'
  | 'err_server'
  | 'err_decode'

export type HaltKind =
  | 'token_invalid'
  | 'token_mismatch'
  | 'revoked'
  | 'archived'
  | 'auto_disabled'
  | 'not_found'
  | 'unavailable'

export interface Halt {
  kind: HaltKind
  message: string
}

export interface LocalCheck {
  url: string
  expect_min: number
  expect_max: number
  timeout_secs: number
}

export interface SparkPoint {
  ok: boolean
  latency_ms: number | null
}

export interface AgentStats {
  total: number
  ok: number
  success_rate: number
  avg_latency_ms: number
}

export interface AgentView {
  id: string
  name: string
  fingerprint: string
  in_service: boolean
  interval_secs: number
  local_check: LocalCheck | null
  added_at: number
  in_flight: boolean
  consecutive_failures: number
  next_beat_at: number | null
  last_attempt_at: number | null
  last_success_at: number | null
  last_kind: BeatKind | null
  last_note: string | null
  last_latency_ms: number | null
  platform_status: string | null
  platform_last_heartbeat: string | null
  halted: Halt | null
  local_check_failing: boolean
  stats_24h: AgentStats
  stats_7d: AgentStats
  spark: SparkPoint[]
}

export interface ActivityEvent {
  ts: number
  level: 'info' | 'warn' | 'error'
  agent_id?: string
  agent_name?: string
  text: string
}

export interface Snapshot {
  version: string
  vault_state: VaultState
  vault_mode: VaultMode | null
  paused: boolean
  base_url: string
  theme: 'system' | 'dark' | 'light'
  minimize_to_tray: boolean
  notify_on_problems: boolean
  default_interval_secs: number
  autostart_enabled: boolean
  tray: TraySeverity
  now: number
  agents: AgentView[]
  activity: ActivityEvent[]
}

export interface BeatRecord {
  ts: number
  kind: BeatKind
  http_status?: number
  latency_ms?: number
  note?: string
}

export interface AgentSettingsPatch {
  interval_secs?: number
  local_check?: LocalCheck | null
}

export interface SettingsPatch {
  base_url?: string
  theme?: string
  minimize_to_tray?: boolean
  notify_on_problems?: boolean
  default_interval_secs?: number
}
