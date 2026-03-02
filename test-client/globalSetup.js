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

const mode      = process.env.__TEST_MODE ?? 'test'
const ROOT      = path.resolve(process.cwd(), '..')
const clientEnv = loadEnv(mode, process.cwd(), '')  // test-client/.env.test → SUPABASE_*
const serverEnv = loadEnv(mode, ROOT, '')            // root .env.test → SUPARUST_*

const BASE        = clientEnv.SUPABASE_URL
const SERVICE_KEY = clientEnv.SUPABASE_SERVICE_KEY
const TEST_EMAIL  = clientEnv.TEST_EMAIL
const TEST_PASS   = clientEnv.TEST_PASSWORD

let serverProcess = null
let serverExited  = false

// ── HTTP health check: retry until /auth/v1/health returns 200 ─────────────
// Uses a shared abort flag so server exit can cancel the polling loop.
async function waitForServer(baseUrl, timeout = 300_000) {
  const start = Date.now()
  let warned = false
  while (Date.now() - start < timeout) {
    if (serverExited) throw new Error(`[globalSetup] Server exited before becoming ready`)
    try {
      const res = await fetch(`${baseUrl}/auth/v1/health`)
      if (res.ok) return
    } catch {}
    if (!warned && Date.now() - start > 30_000) {
      console.log('[globalSetup] Still waiting... first run downloads pg-embed binary (~50MB), this may take a few minutes.')
      warned = true
    }
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
  serverProcess = spawn('cargo', ['run', '--', 'start'], {
    cwd: ROOT,
    env: { ...process.env, ...serverEnv },  // root .env.test: SUPARUST_PORT, SUPARUST_DB_DATA_DIR, etc.
    shell: true,
    stdio: ['ignore', 'pipe', 'pipe'],  // pipe both stdout+stderr — TracingWriter::Stdout uses stdout
  })

  // Server logs go to stdout (TracingWriter::Stdout)
  serverProcess.stdout.on('data', d => {
    const line = d.toString().trim()
    if (line.includes('Listening') || line.includes('WARN') || line.includes('ERROR') || line.includes('error')) {
      console.log(`[server] ${line}`)
    }
  })

  // Cargo build output goes to stderr
  serverProcess.stderr.on('data', d => {
    const line = d.toString().trim()
    if (line.includes('error[')) {
      console.log(`[cargo] ${line}`)
    }
  })

  serverProcess.on('exit', code => {
    serverExited = true
    if (code !== null && code !== 0) {
      console.error(`[globalSetup] Server exited with code ${code} — check output above for details`)
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
