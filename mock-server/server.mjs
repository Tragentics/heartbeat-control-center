// Tragentics API mock — local stand-in for the two endpoints the Heartbeat
// Control Center uses. NEVER touches the real platform; exists so the app can
// be exercised end-to-end without writing anything to production.
//
//   GET  /api/agents/me                      → agent object (bare), like ok(agent)
//   POST /api/agents/:id/heartbeat           → { heartbeat: 'accepted', agent: {...} }
//   Errors → { error: message } with matching status, like err(message, status)
//
// Scenario tokens (paste into the app against base URL http://127.0.0.1:4571):
//   token #1  Mock Invoice Parser   — always succeeds
//   token #2  Mock Support Router   — always succeeds (slower)
//   token #3  Flaky Gateway         — every 3rd heartbeat returns 500
//   token #4  Rate Limited          — every 2nd heartbeat returns 429 (Retry-After: 7)
//   token #5  Dies After Three      — 401 "Your agent has been revoked" after 3 beats
//   token #6  Timeout Tester        — heartbeat hangs 20s (client times out at 15s)
//
// Usage: node server.mjs   (PORT env var optional, default 4571)

import { createServer } from 'http'
import { randomBytes } from 'crypto'

const PORT = Number(process.env.PORT ?? 4571)

const hex = (n) => randomBytes(n).toString('hex')
const uuid = () =>
  `${hex(4)}-${hex(2)}-4${hex(2).slice(1)}-8${hex(2).slice(1)}-${hex(6)}`

function makeAgent(name, behavior) {
  const token = `tk_${hex(32)}`
  return {
    token,
    behavior,
    beatCount: 0,
    record: {
      id: uuid(),
      name,
      slug: name.toLowerCase().replace(/[^a-z0-9]+/g, '-'),
      status: 'offline',
      is_public: false,
      is_revoked: false,
      is_archived: false,
      last_heartbeat: null,
      created_at: new Date().toISOString(),
    },
  }
}

const agents = [
  makeAgent('Mock Invoice Parser', 'ok'),
  makeAgent('Mock Support Router', 'slow'),
  makeAgent('Flaky Gateway', 'flaky500'),
  makeAgent('Rate Limited', 'flaky429'),
  makeAgent('Dies After Three', 'dies'),
  makeAgent('Timeout Tester', 'timeout'),
]

const byToken = new Map(agents.map((a) => [a.token, a]))
const byId = new Map(agents.map((a) => [a.record.id, a]))

function send(res, status, body, headers = {}) {
  const json = JSON.stringify(body)
  res.writeHead(status, { 'content-type': 'application/json', ...headers })
  res.end(json)
}

function bearer(req) {
  const h = req.headers.authorization ?? ''
  return h.startsWith('Bearer ') ? h.slice(7) : null
}

const sleep = (ms) => new Promise((r) => setTimeout(r, ms))

const server = createServer(async (req, res) => {
  const url = new URL(req.url, `http://127.0.0.1:${PORT}`)
  const stamp = new Date().toISOString().slice(11, 19)

  // GET /api/agents/me
  if (req.method === 'GET' && url.pathname === '/api/agents/me') {
    const token = bearer(req)
    const agent = token ? byToken.get(token) : null
    if (!agent) {
      console.log(`[${stamp}] GET /me → 401 (unknown token)`)
      return send(res, 401, { error: 'Missing or invalid Authorization header' })
    }
    if (agent.behavior === 'dies' && agent.beatCount >= 3) {
      console.log(`[${stamp}] GET /me (${agent.record.name}) → 401 revoked`)
      return send(res, 401, { error: 'Your agent has been revoked' })
    }
    console.log(`[${stamp}] GET /me → 200 (${agent.record.name})`)
    return send(res, 200, agent.record)
  }

  // POST /api/agents/:id/heartbeat
  const hb = url.pathname.match(/^\/api\/agents\/([^/]+)\/heartbeat$/)
  if (req.method === 'POST' && hb) {
    const token = bearer(req)
    const agent = token ? byToken.get(token) : null
    if (!agent) {
      console.log(`[${stamp}] POST heartbeat → 401 (unknown token)`)
      return send(res, 401, { error: 'Missing or invalid Authorization header' })
    }
    if (agent.record.id !== hb[1]) {
      console.log(`[${stamp}] POST heartbeat → 403 (token/agent mismatch)`)
      return send(res, 403, { error: 'Unauthorized — your API key does not match the requested agent' })
    }

    let body = ''
    for await (const chunk of req) body += chunk
    let status = 'online'
    try {
      status = JSON.parse(body || '{}').status ?? 'online'
    } catch {
      /* platform defaults to online on unparseable body */
    }
    if (!['online', 'offline'].includes(status)) {
      return send(res, 400, { error: 'Invalid status — must be "online" or "offline"' })
    }

    agent.beatCount++
    const n = agent.beatCount
    const name = agent.record.name

    switch (agent.behavior) {
      case 'slow':
        await sleep(1200)
        break
      case 'flaky500':
        if (n % 3 === 0) {
          console.log(`[${stamp}] POST heartbeat (${name}) #${n} → 500`)
          return send(res, 500, { error: 'Internal server error' })
        }
        break
      case 'flaky429':
        if (n % 2 === 0) {
          console.log(`[${stamp}] POST heartbeat (${name}) #${n} → 429`)
          return send(res, 429, { error: 'Too many requests' }, { 'retry-after': '7' })
        }
        break
      case 'dies':
        if (n > 3) {
          console.log(`[${stamp}] POST heartbeat (${name}) #${n} → 401 revoked`)
          return send(res, 401, { error: 'Your agent has been revoked' })
        }
        break
      case 'timeout':
        console.log(`[${stamp}] POST heartbeat (${name}) #${n} → hanging 20s…`)
        await sleep(20_000)
        break
      default:
        break
    }

    agent.record.status = status
    agent.record.last_heartbeat = new Date().toISOString()
    console.log(`[${stamp}] POST heartbeat (${name}) #${n} → 200 ${status}`)
    return send(res, 200, {
      heartbeat: 'accepted',
      agent: { id: agent.record.id, status, last_heartbeat: agent.record.last_heartbeat },
    })
  }

  send(res, 404, { error: 'Not found' })
})

server.listen(PORT, '127.0.0.1', () => {
  console.log(`Tragentics mock API listening on http://127.0.0.1:${PORT}`)
  console.log('')
  console.log('In the app: Settings → Connection → base URL http://127.0.0.1:4571')
  console.log('')
  console.log('Scenario tokens (paste into Add Agent):')
  for (const a of agents) {
    console.log(`  ${a.record.name.padEnd(22)} ${a.token}   (${a.behavior})`)
  }
})
