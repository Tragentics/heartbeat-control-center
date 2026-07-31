// Settings tab.

import { escapeHtml } from '../lib/format'
import { icons } from '../lib/icons'
import type { Snapshot } from '../lib/types'
import { switchControl } from './components'

export function renderSettings(snap: Snapshot): string {
  const isPassphrase = snap.vault_mode === 'passphrase'
  return `<div class="content-inner">
    <span class="section-label">Settings</span>

    <div class="card">
      <div class="card-header"><div><div class="card-title">General</div></div></div>
      <div style="margin-top:10px">
        <div class="settings-row">
          <div class="sr-main"><div class="sr-title">Launch at login</div><div class="sr-desc">Start hidden in the tray when you sign in — beats begin without opening the window.</div></div>
          <div class="sr-control">${switchControl('toggle-autostart', 'autostart', snap.autostart_enabled, 'Launch at login')}</div>
        </div>
        <div class="settings-row">
          <div class="sr-main"><div class="sr-title">Close button hides to tray</div><div class="sr-desc">Closing the window keeps the engine beating in the background. Quit from the tray menu.</div></div>
          <div class="sr-control">${switchControl('toggle-minimize', 'minimize', snap.minimize_to_tray, 'Close hides to tray')}</div>
        </div>
        <div class="settings-row">
          <div class="sr-main"><div class="sr-title">Notify on problems</div><div class="sr-desc">System notification when an agent halts, keeps failing, or fails its local health check.</div></div>
          <div class="sr-control">${switchControl('toggle-notify', 'notify', snap.notify_on_problems, 'Notifications')}</div>
        </div>
        <div class="settings-row">
          <div class="sr-main"><div class="sr-title">Theme</div><div class="sr-desc">Follows the Tragentics design system in both modes.</div></div>
          <div class="sr-control"><div class="seg">
            <button data-action="set-theme" data-id="system" class="${snap.theme === 'system' ? 'active' : ''}">${icons.monitor} System</button>
            <button data-action="set-theme" data-id="dark" class="${snap.theme === 'dark' ? 'active' : ''}">${icons.moon} Dark</button>
            <button data-action="set-theme" data-id="light" class="${snap.theme === 'light' ? 'active' : ''}">${icons.sun} Light</button>
          </div></div>
        </div>
      </div>
    </div>

    <div class="card">
      <div class="card-header"><div><div class="card-title">Beating</div><div class="card-sub">How the metronome paces your fleet</div></div></div>
      <div style="margin-top:10px">
        <div class="settings-row">
          <div class="sr-main"><div class="sr-title">Default interval for new agents</div><div class="sr-desc">5 minutes is the floor — Beat now covers on-demand beats. Tragentics marks agents idle after 15 quiet minutes, so every option keeps a safe margin. Per-agent override in the agent's details.</div></div>
          <div class="sr-control">
            <select class="input" id="default-interval" style="width:130px">
              ${[300, 480, 600, 840].map((s) => `<option value="${s}" ${snap.default_interval_secs === s ? 'selected' : ''}>${s / 60} min</option>`).join('')}
            </select>
          </div>
        </div>
        <div class="settings-row">
          <div class="sr-main"><div class="sr-title">Pacing &amp; limits</div><div class="sr-desc">Beats are jittered ±10% and launched at most 2 per second, 4 in flight — well inside the platform's 120 requests/min per agent and per network. Failures back off exponentially; rate limits honor Retry-After.</div></div>
          <div class="sr-control"><span class="badge badge-muted">automatic</span></div>
        </div>
      </div>
    </div>

    <div class="card">
      <div class="card-header"><div><div class="card-title">Connection</div></div></div>
      <div class="card-body" style="display:flex;flex-direction:column;gap:12px">
        <div class="field">
          <label for="base-url">Tragentics API base URL <span class="hint">— leave as-is unless you're testing against the local mock server</span></label>
          <div style="display:flex;gap:8px">
            <input id="base-url" class="input mono" value="${escapeHtml(snap.base_url)}" spellcheck="false" />
            <button class="btn btn-outline" data-action="test-base-url">Test</button>
            <button class="btn btn-primary" data-action="save-base-url">Save</button>
          </div>
          <div id="base-url-result"></div>
        </div>
      </div>
    </div>

    <div class="card">
      <div class="card-header"><div><div class="card-title">Local Vault</div><div class="card-sub">AES-256-GCM encrypted token store on this device</div></div></div>
      <div style="margin-top:10px">
        <div class="settings-row">
          <div class="sr-main"><div class="sr-title">Protection mode</div><div class="sr-desc">${
            isPassphrase
              ? 'Passphrase (Argon2id) — the key is derived when you unlock, never stored.'
              : 'System keychain — the vault key lives in your OS credential store.'
          }</div></div>
          <div class="sr-control"><span class="badge badge-muted">${isPassphrase ? 'passphrase' : 'system keychain'}</span></div>
        </div>
        ${
          isPassphrase
            ? `<div class="settings-row">
                <div class="sr-main"><div class="sr-title">Lock now</div><div class="sr-desc">Drops the key and tokens from memory. Beating pauses until you unlock.</div></div>
                <div class="sr-control"><button class="btn btn-outline btn-sm" data-action="lock-vault">${icons.lock} Lock vault</button></div>
              </div>
              <div class="settings-row">
                <div class="sr-main"><div class="sr-title">Change passphrase</div><div class="sr-desc">Re-encrypts the vault under a fresh key and salt.</div></div>
                <div class="sr-control"><button class="btn btn-outline btn-sm" data-action="open-passphrase">${icons.key} Change…</button></div>
              </div>`
            : ''
        }
        <div class="settings-row">
          <div class="sr-main"><div class="sr-title">Data folder</div><div class="sr-desc">config.json &amp; history.json are plain; vault.bin is encrypted. Tokens never appear anywhere else.</div></div>
          <div class="sr-control"><button class="btn btn-ghost btn-sm" data-action="open-data-dir">${icons.folder} Reveal</button></div>
        </div>
      </div>
    </div>

    <div class="card">
      <div class="card-header"><div><div class="card-title">About</div></div></div>
      <div style="margin-top:10px">
        <div class="settings-row">
          <div class="sr-main"><div class="sr-title">Tragentics Heartbeat Control Center</div><div class="sr-desc">Version ${escapeHtml(snap.version)} · the window is a pure renderer — only the Rust core ever talks to the network, and only to your configured Tragentics API.</div></div>
          <div class="sr-control"><button class="btn btn-ghost btn-sm" data-action="open-site">${icons.external} tragentics.com</button></div>
        </div>
        <div class="settings-row">
          <div class="sr-main"><div class="sr-title">Quit</div><div class="sr-desc">Reports every in-service agent offline first (honest shutdown), then exits.</div></div>
          <div class="sr-control"><button class="btn btn-danger btn-sm" data-action="quit-app">${icons.power} Quit</button></div>
        </div>
      </div>
    </div>
  </div>`
}
