// Vault gate screens: first-run setup, passphrase unlock, keyring trouble.

import { icons } from '../lib/icons'
import type { Snapshot } from '../lib/types'

export function renderGate(snap: Snapshot, selectedMode: 'keyring' | 'passphrase'): string {
  if (snap.vault_state === 'uninitialized') return renderSetup(selectedMode)
  if (snap.vault_state === 'locked') return renderUnlock()
  return renderKeyringTrouble()
}

function renderSetup(mode: 'keyring' | 'passphrase'): string {
  return `<div class="gate"><div class="gate-card">
    <div class="gate-icon">${icons.vault}</div>
    <div>
      <h2>Set up your Local Vault</h2>
      <p class="gate-sub">Agent tokens are encrypted on this device with AES-256-GCM. They are only ever sent to Tragentics to authenticate your agents — never anywhere else.</p>
    </div>
    <div class="mode-pick">
      <button class="mode-card ${mode === 'keyring' ? 'selected' : ''}" data-action="pick-mode" data-id="keyring">
        <span class="mode-title">${icons.shield} System keychain <span class="badge badge-online" style="text-transform:none">Recommended</span></span>
        <span class="mode-desc">The vault key is stored in your operating system's credential store (Windows Credential Manager / macOS Keychain). Unlocks automatically when you log in — nothing extra to remember.</span>
      </button>
      <button class="mode-card ${mode === 'passphrase' ? 'selected' : ''}" data-action="pick-mode" data-id="passphrase">
        <span class="mode-title">${icons.key} Passphrase</span>
        <span class="mode-desc">The vault key is derived from a passphrase (Argon2id) you enter each time the app starts. Nothing usable is stored on disk.</span>
      </button>
    </div>
    ${
      mode === 'passphrase'
        ? `<div class="field">
            <label for="setup-pass">Passphrase <span class="hint">(minimum 10 characters)</span></label>
            <input id="setup-pass" class="input" type="password" autocomplete="new-password" placeholder="Choose a strong passphrase" />
          </div>
          <div class="field">
            <label for="setup-pass2">Confirm passphrase</label>
            <input id="setup-pass2" class="input" type="password" autocomplete="new-password" placeholder="Type it again" />
          </div>`
        : ''
    }
    <div id="gate-error"></div>
    <button class="btn btn-primary" data-action="create-vault">${icons.vault} Create vault</button>
  </div></div>`
}

function renderUnlock(): string {
  return `<div class="gate"><div class="gate-card">
    <div class="gate-icon">${icons.lock}</div>
    <div>
      <h2>Vault locked</h2>
      <p class="gate-sub">Enter your passphrase to unlock the vault and resume beating. Agents stay paused until the vault is open.</p>
    </div>
    <div class="field">
      <label for="unlock-pass">Passphrase</label>
      <input id="unlock-pass" class="input" type="password" autocomplete="current-password" placeholder="Your vault passphrase" autofocus />
    </div>
    <div id="gate-error"></div>
    <button class="btn btn-primary" data-action="unlock-vault">${icons.unlock} Unlock</button>
  </div></div>`
}

function renderKeyringTrouble(): string {
  return `<div class="gate"><div class="gate-card">
    <div class="gate-icon" style="color:var(--status-idle)">${icons.alert}</div>
    <div>
      <h2>Can't reach the system keychain</h2>
      <p class="gate-sub">The vault exists, but its key could not be read from your operating system's credential store. This can happen after an OS reinstall or profile change. Beating is suspended until the key is available again.</p>
    </div>
    <div class="kv">
      <dt>What you can try</dt>
      <dd>Restart the app, or check that your OS credential store is unlocked.</dd>
      <dt>Worst case</dt>
      <dd>If the key is gone for good, remove the vault file and re-add your agents. Agent tokens are shown only once at registration — if a token wasn't saved elsewhere, revoke and re-register that agent on tragentics.com.</dd>
    </div>
    <div style="display:flex;gap:8px">
      <button class="btn btn-outline" data-action="retry-vault">${icons.refresh} Retry</button>
      <button class="btn btn-ghost" data-action="open-data-dir">${icons.folder} Open data folder</button>
    </div>
  </div></div>`
}
