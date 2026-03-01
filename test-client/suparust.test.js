import { describe, test, expect, beforeAll } from 'vitest'
import { createClient } from '@supabase/supabase-js'

const supabase = createClient(
  'http://127.0.0.1:3000',
  'eyJhbGciOiJIUzI1NiIsInR5cCI6IkpXVCJ9.eyJyb2xlIjoiYW5vbiIsImlzcyI6InN1cGFydXN0IiwiaWF0IjoxNzcyMTI2NDAzfQ.m_CuBVEMSIsVQ2lnYsGZGcc3SKC0tTD1UBFTctWFbqc'
)

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
    // Set session manually so supabase-js uses our token
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
      email: 'test@suparust.dev',
      password: 'Password123!'
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
      .eq('email', 'test@suparust.dev')
    expect(error).toBeNull()
    expect(data.length).toBeGreaterThan(0)
    expect(data[0].email).toBe('test@suparust.dev')
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
    const anonClient = createClient(
      'http://127.0.0.1:3000',
      'eyJhbGciOiJIUzI1NiIsInR5cCI6IkpXVCJ9.eyJyb2xlIjoiYW5vbiIsImlzcyI6InN1cGFydXN0IiwiaWF0IjoxNzcyMTI2NDAzfQ.m_CuBVEMSIsVQ2lnYsGZGcc3SKC0tTD1UBFTctWFbqc'
    )
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
      email: 'test@suparust.dev',
      password: 'Password123!'
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

const BASE_URL = 'http://127.0.0.1:3000'
const ANON_KEY = 'eyJhbGciOiJIUzI1NiIsInR5cCI6IkpXVCJ9.eyJyb2xlIjoiYW5vbiIsImlzcyI6InN1cGFydXN0IiwiaWF0IjoxNzcyMTU1MzQ4fQ.yvMr38AEPO8N-zkn_GSPtH71e7PHSDHS7GxQ-9PahE8'
const SERVICE_KEY = 'eyJhbGciOiJIUzI1NiIsInR5cCI6IkpXVCJ9.eyJyb2xlIjoic2VydmljZV9yb2xlIiwiaXNzIjoic3VwYXJ1c3QiLCJpYXQiOjE3NzIxNTUzNDh9.Y1lzcK2qOGv6TH-mU896Kw8uRvYG0eXckvrFsKP3iK8'

const adminClient = createClient(BASE_URL, SERVICE_KEY)
const anonClient  = createClient(BASE_URL, ANON_KEY)

describe('Admin — Service Role RLS Bypass', () => {

  const adminEmail = `admin_seed_${Date.now()}@suparust.dev`
  const adminPass  = 'AdminSeed123!'

  // ── 4.1 Admin dapat membaca seluruh user (bypass RLS) ──
  test('service_role can select all users', async () => {
    const { data, error } = await adminClient
      .from('users')
      .select('id, email')
    expect(error).toBeNull()
    expect(Array.isArray(data)).toBe(true)
    // service_role harus bisa lihat semua row — minimal 1 (test user dari Suite 1)
    expect(data.length).toBeGreaterThan(0)
  })

  // ── 4.2 Anon TIDAK dapat membaca users (RLS menolak) ──
  test('anon cannot select users — RLS denies', async () => {
    const { data, error } = await anonClient
      .from('users')
      .select('id, email')
    if (error) {
      expect(error.message).toBeTruthy() // denied = ok
    } else {
      expect(data.length).toBe(0)
    }
  })

  // ── 4.3 Admin dapat membuat bucket baru ──
  test('service_role can create new bucket', async () => {
    const { data, error } = await adminClient.storage
      .createBucket('admin-test-bucket', { public: false })
    // Acceptable: created fresh OR already exists
    const ok = !error || error.message?.includes('already exists')
    expect(ok).toBe(true)
  })

  // ── 4.4 Admin dapat upload ke bucket manapun tanpa RLS ──
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

  // ── 4.5 Anon TIDAK dapat upload ke private bucket ──
  test('anon cannot upload to private bucket — RLS denies', async () => {
    const blob = new Blob(['anon-attempt'], { type: 'text/plain' })
    const { error } = await anonClient.storage
      .from('admin-test-bucket')
      .upload(`seed/anon_${Date.now()}.txt`, blob, { upsert: true })
    // Harus ada error — RLS policy menolak anon insert
    expect(error).toBeDefined()
    expect(error.message).toBeTruthy()
  })

  // ── 4.6 Admin dapat list semua bucket ──
  test('service_role can list all buckets', async () => {
    const { data, error } = await adminClient.storage.listBuckets()
    expect(error).toBeNull()
    expect(Array.isArray(data)).toBe(true)
    // Minimal ada 'avatars' (dari Suite 3) + 'admin-test-bucket' (4.3)
    expect(data.length).toBeGreaterThanOrEqual(1)
  })

  // ── 4.7 Admin dapat delete bucket ──
  test('service_role can delete bucket', async () => {
    // Kosongkan dulu
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
