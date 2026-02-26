CREATE SCHEMA IF NOT EXISTS auth;

CREATE TABLE IF NOT EXISTS auth.users (
  id                    uuid PRIMARY KEY DEFAULT gen_random_uuid(),
  email                 text UNIQUE,
  encrypted_password    text,
  email_confirmed_at    timestamptz,
  phone                 text UNIQUE,
  phone_confirmed_at    timestamptz,
  raw_app_meta_data     jsonb DEFAULT '{"provider":"email","providers":["email"]}',
  raw_user_meta_data    jsonb DEFAULT '{}',
  role                  text DEFAULT 'authenticated',
  confirmation_token    text,
  recovery_token        text,
  email_change_token_new text,
  last_sign_in_at       timestamptz,
  created_at            timestamptz DEFAULT now(),
  updated_at            timestamptz DEFAULT now()
);

CREATE TABLE IF NOT EXISTS auth.sessions (
  id         uuid PRIMARY KEY DEFAULT gen_random_uuid(),
  user_id    uuid NOT NULL REFERENCES auth.users(id) ON DELETE CASCADE,
  created_at timestamptz DEFAULT now(),
  updated_at timestamptz DEFAULT now(),
  factor_id  uuid,
  aal        text DEFAULT 'aal1'
);

CREATE TABLE IF NOT EXISTS auth.refresh_tokens (
  id         bigint GENERATED ALWAYS AS IDENTITY PRIMARY KEY,
  token      text UNIQUE NOT NULL,
  user_id    uuid NOT NULL REFERENCES auth.users(id) ON DELETE CASCADE,
  session_id uuid REFERENCES auth.sessions(id) ON DELETE CASCADE,
  parent     text,
  revoked    boolean DEFAULT false,
  created_at timestamptz DEFAULT now(),
  updated_at timestamptz DEFAULT now()
);

CREATE TABLE IF NOT EXISTS auth.identities (
  id              text NOT NULL,
  user_id         uuid NOT NULL REFERENCES auth.users(id) ON DELETE CASCADE,
  identity_data   jsonb NOT NULL,
  provider        text NOT NULL,
  last_sign_in_at timestamptz,
  created_at      timestamptz DEFAULT now(),
  PRIMARY KEY (provider, id)
);

-- GoTrue Postgres functions (WAJIB untuk RLS compatibility)
CREATE OR REPLACE FUNCTION auth.uid() RETURNS uuid
  LANGUAGE sql STABLE AS $$
    SELECT COALESCE(
      current_setting('request.jwt.claims', true)::jsonb->>'sub', NULL
    )::uuid
  $$;

CREATE OR REPLACE FUNCTION auth.role() RETURNS text
  LANGUAGE sql STABLE AS $$
    SELECT COALESCE(
      current_setting('request.jwt.claims', true)::jsonb->>'role', 'anon'
    )::text
  $$;

CREATE OR REPLACE FUNCTION auth.jwt() RETURNS jsonb
  LANGUAGE sql STABLE AS $$
    SELECT COALESCE(
      current_setting('request.jwt.claims', true)::jsonb, '{}'::jsonb
    )
  $$;
