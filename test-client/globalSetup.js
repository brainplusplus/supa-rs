/**
 * Vitest global setup — runs once before all test files.
 * Creates the seeded test user and avatars bucket via the API.
 */

const BASE = 'http://127.0.0.1:3000'
const SERVICE_KEY = 'eyJhbGciOiJIUzI1NiIsInR5cCI6IkpXVCJ9.eyJyb2xlIjoic2VydmljZV9yb2xlIiwiaXNzIjoic3VwYXJ1c3QiLCJpYXQiOjE3NzIxNTUzNDh9.Y1lzcK2qOGv6TH-mU896Kw8uRvYG0eXckvrFsKP3iK8'

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
  // 1. Create test@suparust.dev via signup endpoint
  const signupRes = await apiPost('/auth/v1/signup', {
    email: 'test@suparust.dev',
    password: 'Password123!',
  })
  const signupData = await signupRes.json()
  // 409 / "User already registered" is fine
  if (signupRes.ok) {
    console.log('[globalSetup] Created test@suparust.dev')
  } else {
    console.log('[globalSetup] test@suparust.dev already exists (ok)')
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
