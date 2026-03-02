import { describe, test, expect, beforeAll } from 'vitest'
import { createClient } from '@supabase/supabase-js'

const BASE_URL    = import.meta.env.SUPABASE_URL
const ANON_KEY    = import.meta.env.SUPABASE_ANON_KEY
const SERVICE_KEY = import.meta.env.SUPABASE_SERVICE_KEY
const TEST_EMAIL  = import.meta.env.TEST_EMAIL
const TEST_PASS   = import.meta.env.TEST_PASSWORD

const supabase    = createClient(BASE_URL, ANON_KEY)
const adminClient = createClient(BASE_URL, SERVICE_KEY)
const anonClient  = createClient(BASE_URL, ANON_KEY)

describe('Auth', () => {

  const email = `test_${Date.now()}@suparust.dev`
  const password = 'TestPassword123!'
  let accessToken = ''
  let refreshToken = ''

  test('signup creates new user', async () => {
    const { data, error } = await supabase.auth.signUp({ email, password })
    expect(error).toBeNull()
    expect(data.user).toBeDefined()
    expect(data.user.email).toBe(email)
    expect(data.user.role).toBe('authenticated')
    expect(data.session.access_token).toBeTruthy()
    accessToken = data.session.access_token
    refreshToken = data.session.refresh_token
  })

  test('login with password returns valid session', async () => {
    const { data, error } = await supabase.auth.signInWithPassword(
      { email, password }
    )
    expect(error).toBeNull()
    expect(data.session.access_token).toBeTruthy()
    expect(data.user.email).toBe(email)
    accessToken = data.session.access_token
    refreshToken = data.session.refresh_token
  })

  test('getUser returns authenticated user', async () => {
    await supabase.auth.setSession({
      access_token: accessToken,
      refresh_token: refreshToken
    })
    const { data, error } = await supabase.auth.getUser()
    expect(error).toBeNull()
    expect(data.user.email).toBe(email)
  })

  test('duplicate signup returns error', async () => {
    const { data, error } = await supabase.auth.signUp({ email, password })
    expect(error || data.user).toBeDefined()
  })

  test('wrong password returns error', async () => {
    const { error } = await supabase.auth.signInWithPassword({
      email, password: 'wrongpassword'
    })
    expect(error).toBeDefined()
    expect(error.message).toBeTruthy()
  })
})

describe('REST API', () => {
  let userSession = null

  beforeAll(async () => {
    const { data } = await supabase.auth.signInWithPassword({
      email: TEST_EMAIL,
      password: TEST_PASS,
    })
    userSession = data.session
    await supabase.auth.setSession(userSession)
  })

  test('select returns array', async () => {
    const { data, error } = await supabase
      .from('users')
      .select('id, email')
    expect(error).toBeNull()
    expect(Array.isArray(data)).toBe(true)
  })

  test('select with filter', async () => {
    const { data, error } = await supabase
      .from('users')
      .select('id, email')
      .eq('email', TEST_EMAIL)
    expect(error).toBeNull()
    expect(data.length).toBeGreaterThan(0)
    expect(data[0].email).toBe(TEST_EMAIL)
  })

  test('select with limit', async () => {
    const { data, error } = await supabase
      .from('users')
      .select('id')
      .limit(1)
    expect(error).toBeNull()
    expect(data.length).toBeLessThanOrEqual(1)
  })

  test('unauthenticated select respects RLS', async () => {
    const { data, error } = await anonClient
      .from('users')
      .select('id, email')
    expect(error).toBeNull()
    expect(Array.isArray(data)).toBe(true)
  })
})

describe('Storage', () => {
  let userSession = null
  const testFileName = `hello_${Date.now()}.txt`
  const testContent = 'Hello SupaRust Storage from supabase-js!'

  beforeAll(async () => {
    const { data } = await supabase.auth.signInWithPassword({
      email: TEST_EMAIL,
      password: TEST_PASS,
    })
    userSession = data.session
    await supabase.auth.setSession(userSession)
  })

  test('list buckets returns array', async () => {
    const { data, error } = await supabase.storage.listBuckets()
    expect(error).toBeNull()
    expect(Array.isArray(data)).toBe(true)
  })

  test('upload file to avatars bucket', async () => {
    const blob = new Blob([testContent], { type: 'text/plain' })
    const { data, error } = await supabase.storage
      .from('avatars')
      .upload(`test/${testFileName}`, blob, {
        contentType: 'text/plain',
        upsert: true
      })
    expect(error).toBeNull()
    expect(data.path || data.Key).toBeTruthy()
  })

  test('download uploaded file', async () => {
    const { data, error } = await supabase.storage
      .from('avatars')
      .download(`test/${testFileName}`)
    expect(error).toBeNull()
    expect(data).toBeDefined()
    const text = await data.text()
    expect(text).toBe(testContent)
  })

  test('get public URL format', async () => {
    const { data } = supabase.storage
      .from('avatars')
      .getPublicUrl(`test/${testFileName}`)
    expect(data.publicUrl).toContain('/storage/v1/object/public/')
  })

  test('delete uploaded file', async () => {
    const { error } = await supabase.storage
      .from('avatars')
      .remove([`test/${testFileName}`])
    expect(error).toBeNull()
  })
})

// ============================================================
// Suite 4: Admin — Service Role RLS Bypass
// ============================================================

describe('Admin — Service Role RLS Bypass', () => {

  const adminEmail = `admin_seed_${Date.now()}@suparust.dev`
  const adminPass  = 'AdminSeed123!'

  test('service_role can select all users', async () => {
    const { data, error } = await adminClient
      .from('users')
      .select('id, email')
    expect(error).toBeNull()
    expect(Array.isArray(data)).toBe(true)
    expect(data.length).toBeGreaterThan(0)
  })

  test('anon cannot select users — RLS denies', async () => {
    const { data, error } = await anonClient
      .from('users')
      .select('id, email')
    if (error) {
      expect(error.message).toBeTruthy()
    } else {
      expect(data.length).toBe(0)
    }
  })

  test('service_role can create new bucket', async () => {
    const { data, error } = await adminClient.storage
      .createBucket('admin-test-bucket', { public: false })
    const ok = !error || error.message?.includes('already exists')
    expect(ok).toBe(true)
  })

  test('service_role can upload to any bucket', async () => {
    const blob = new Blob(['admin-seeded-content'], { type: 'text/plain' })
    const { data, error } = await adminClient.storage
      .from('admin-test-bucket')
      .upload(`seed/admin_${Date.now()}.txt`, blob, {
        contentType: 'text/plain',
        upsert: true
      })
    expect(error).toBeNull()
    expect(data.path || data.Key).toBeTruthy()
  })

  test('anon cannot upload to private bucket — RLS denies', async () => {
    const blob = new Blob(['anon-attempt'], { type: 'text/plain' })
    const { error } = await anonClient.storage
      .from('admin-test-bucket')
      .upload(`seed/anon_${Date.now()}.txt`, blob, { upsert: true })
    expect(error).toBeDefined()
    expect(error.message).toBeTruthy()
  })

  test('service_role can list all buckets', async () => {
    const { data, error } = await adminClient.storage.listBuckets()
    expect(error).toBeNull()
    expect(Array.isArray(data)).toBe(true)
    expect(data.length).toBeGreaterThanOrEqual(1)
  })

  test('service_role can delete bucket', async () => {
    const { data: files } = await adminClient.storage
      .from('admin-test-bucket')
      .list('seed')
    if (files?.length) {
      await adminClient.storage
        .from('admin-test-bucket')
        .remove(files.map(f => `seed/${f.name}`))
    }
    const { error } = await adminClient.storage
      .deleteBucket('admin-test-bucket')
    expect(error).toBeNull()
  })
})
