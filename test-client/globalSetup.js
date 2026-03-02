/**
 * Vitest global setup — runs once before all test files in Node context.
 * Starts the SupaRust test server, seeds test data, and tears down on completion.
 *
 * Reads config from .env.test via loadEnv (same source as vitest.config.js).
 * Mode is bridged from vitest.config.js via process.env.__TEST_MODE.
 */
import { spawn }   from 'child_process'
import { loadEnv } from 'vite'
import net         from 'net'
import path        from 'path'

const mode = process.env.__TEST_MODE ?? 'test'
const env  = loadEnv(mode, process.cwd(), '')

const BASE        = env.SUPABASE_URL
const SERVICE_KEY = env.SUPABASE_SERVICE_KEY
const TEST_EMAIL  = env.TEST_EMAIL
const TEST_PASS   = env.TEST_PASSWORD
const PORT        = parseInt(env.SUPABASE_URL?.split(':').pop() ?? '53001', 10)

// Repo root is one level up from test-client/
const ROOT = path.resolve(process.cwd(), '..')

let serverProcess = null

// ── TCP health check: retry until port accepts connections ─────────────────
function waitForPort(port, timeout = 90_000) {
  return new Promise((resolve, reject) => {
    const start = Date.now()
    function attempt() {
      const sock = new net.Socket()
      sock.setTimeout(500)
      sock.connect(port, '127.0.0.1', () => {
        sock.destroy()
        resolve()
      })
      sock.on('error', () => {
        sock.destroy()
        if (Date.now() - start > timeout) {
          reject(new Error(`[globalSetup] Server did not start within ${timeout}ms on port ${port}`))
        } else {
          setTimeout(attempt, 500)
        }
      })
      sock.on('timeout', () => {
        sock.destroy()
        setTimeout(attempt, 500)
      })
    }
    attempt()
  })
}

async function apiPost(path, body, token = SERVICE_KEY) {
  const res = await fetch(`${BASE}${path}`, {
    method: 'POST',
    headers: {
      'Content-Type': 'application/json',
      'Authorization': `Bearer ${token}`,
    },
    body: JSON.stringify(body),
  })
  return res
}

// ── Setup: start server + seed data ───────────────────────────────────────
export async function setup() {
  console.log(`[globalSetup] mode=${mode}, url=${BASE}`)

  // Inject test env vars into child process — these win over .env
  // because dotenvy skips vars already present in process env.
  serverProcess = spawn('cargo', ['run'], {
    cwd: ROOT,
    env: { ...process.env, ...env },
    stdio: ['ignore', 'ignore', 'pipe'],  // suppress build noise, show errors
  })

  serverProcess.stderr.on('data', d => {
    const line = d.toString().trim()
    if (line.includes('Listening') || line.includes('error[') || line.includes('WARN') || line.includes('ERROR')) {
      console.log(`[server] ${line}`)
    }
  })

  serverProcess.on('exit', code => {
    if (code !== null && code !== 0) {
      console.error(`[globalSetup] Server exited unexpectedly with code ${code}`)
    }
  })

  console.log(`[globalSetup] Waiting for server on port ${PORT}...`)
  await waitForPort(PORT)
  console.log(`[globalSetup] Server ready at ${BASE}`)

  // Seed: create test user + avatars bucket
  const signupRes = await apiPost('/auth/v1/signup', { email: TEST_EMAIL, password: TEST_PASS })
  if (signupRes.ok) {
    console.log(`[globalSetup] Created ${TEST_EMAIL}`)
  } else {
    console.log(`[globalSetup] ${TEST_EMAIL} already exists (ok)`)
  }

  const bucketRes = await apiPost('/storage/v1/bucket', { id: 'avatars', name: 'avatars', public: true })
  if (bucketRes.ok) {
    console.log('[globalSetup] Created avatars bucket')
  } else {
    console.log('[globalSetup] avatars bucket already exists (ok)')
  }
}

// ── Teardown: stop server after all tests complete ─────────────────────────
export async function teardown() {
  if (serverProcess) {
    console.log('[globalSetup] Stopping test server...')
    serverProcess.kill('SIGTERM')
    await new Promise(r => setTimeout(r, 1500))
    console.log('[globalSetup] Server stopped.')
  }
}
