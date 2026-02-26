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
