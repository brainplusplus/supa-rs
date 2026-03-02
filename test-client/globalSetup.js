/**
 * Vitest global setup — runs once before all test files in Node context.
 * Creates the seeded test user and avatars bucket via the API.
 *
 * Reads config from .env.test via loadEnv (same source as vitest.config.js).
 * Mode is bridged from vitest.config.js via process.env.__TEST_MODE.
 */
import { loadEnv } from 'vite'

const mode = process.env.__TEST_MODE ?? 'test'
const env  = loadEnv(mode, process.cwd(), '')

const BASE        = env.SUPABASE_URL
const SERVICE_KEY = env.SUPABASE_SERVICE_KEY
const TEST_EMAIL  = env.TEST_EMAIL
const TEST_PASS   = env.TEST_PASSWORD

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

export async function setup() {
  console.log(`[globalSetup] mode=${mode}, base=${BASE}`)

  // 1. Create seeded test user via signup endpoint
  const signupRes = await apiPost('/auth/v1/signup', {
    email: TEST_EMAIL,
    password: TEST_PASS,
  })
  if (signupRes.ok) {
    console.log(`[globalSetup] Created ${TEST_EMAIL}`)
  } else {
    console.log(`[globalSetup] ${TEST_EMAIL} already exists (ok)`)
  }

  // 2. Create avatars bucket via storage API
  const bucketRes = await apiPost('/storage/v1/bucket', {
    id: 'avatars',
    name: 'avatars',
    public: true,
  })
  if (bucketRes.ok) {
    console.log('[globalSetup] Created avatars bucket')
  } else {
    console.log('[globalSetup] avatars bucket already exists (ok)')
  }
}
