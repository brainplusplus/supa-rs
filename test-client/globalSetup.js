/**
 * Vitest global setup — runs once before all test files in Node context.
 * Starts the SupaRust test server, seeds test data, and tears down on completion.
 *
 * Reads config from .env.test via loadEnv (same source as vitest.config.js).
 * Mode is bridged from vitest.config.js via process.env.__TEST_MODE.
 */
import { spawn }   from 'child_process'
import { loadEnv } from 'vite'
import path        from 'path'

const mode = process.env.__TEST_MODE ?? 'test'
const env  = loadEnv(mode, process.cwd(), '')

const BASE        = env.SUPABASE_URL
const SERVICE_KEY = env.SUPABASE_SERVICE_KEY
const TEST_EMAIL  = env.TEST_EMAIL
const TEST_PASS   = env.TEST_PASSWORD

// Repo root is one level up from test-client/
const ROOT = path.resolve(process.cwd(), '..')

let serverProcess = null

// ── HTTP health check: retry until /auth/v1/health returns 200 ─────────────
// Using /auth/v1/health (not plain TCP) ensures DB + migrations are ready,
// not just that the port is open.
async function waitForServer(baseUrl, timeout = 90_000) {
  const start = Date.now()
  while (Date.now() - start < timeout) {
    try {
      const res = await fetch(`${baseUrl}/auth/v1/health`)
      if (res.ok) return
    } catch {}
    await new Promise(r => setTimeout(r, 500))
  }
  throw new Error(`[globalSetup] Server did not start within ${timeout}ms at ${baseUrl}`)
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

  console.log(`[globalSetup] Waiting for server at ${BASE}...`)
  await waitForServer(BASE)
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
