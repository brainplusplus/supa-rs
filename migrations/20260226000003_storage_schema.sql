DO $$ 
BEGIN
  IF current_setting('server_version_num')::int < 120000 THEN
    RAISE EXCEPTION 'SupaRust requires PostgreSQL 12+ (for generated columns). Current: %', current_setting('server_version');
  END IF;
END $$;

CREATE SCHEMA IF NOT EXISTS storage;

CREATE TABLE IF NOT EXISTS storage.buckets (
  id                 text PRIMARY KEY,
  name               text UNIQUE NOT NULL,
  owner              uuid REFERENCES auth.users(id),
  public             boolean DEFAULT false,
  file_size_limit    bigint,
  allowed_mime_types text[],
  created_at         timestamptz DEFAULT now(),
  updated_at         timestamptz DEFAULT now()
);

CREATE TABLE IF NOT EXISTS storage.objects (
  id          uuid PRIMARY KEY DEFAULT gen_random_uuid(),
  bucket_id   text REFERENCES storage.buckets(id),
  name        text NOT NULL,
  owner       uuid REFERENCES auth.users(id),
  metadata    jsonb,
  path_tokens text[] GENERATED ALWAYS AS (string_to_array(name, '/')) STORED,
  created_at  timestamptz DEFAULT now(),
  updated_at  timestamptz DEFAULT now(),
  UNIQUE (bucket_id, name)
);

CREATE TABLE IF NOT EXISTS storage.tus_uploads (
  id            uuid PRIMARY KEY DEFAULT gen_random_uuid(),
  bucket_id     text REFERENCES storage.buckets(id),
  object_name   text NOT NULL,
  owner         uuid REFERENCES auth.users(id),
  upload_offset bigint DEFAULT 0,
  upload_length bigint,
  metadata      jsonb,
  expires_at    timestamptz DEFAULT now() + interval '24 hours',
  created_at    timestamptz DEFAULT now()
);

-- Storage helper functions
CREATE OR REPLACE FUNCTION storage.foldername(name text)
RETURNS text[] LANGUAGE sql AS
$$ SELECT string_to_array(name, '/') $$;

CREATE OR REPLACE FUNCTION storage.filename(name text)
RETURNS text LANGUAGE sql AS
$$ SELECT (string_to_array(name, '/'))[array_length(string_to_array(name, '/'), 1)] $$;

CREATE OR REPLACE FUNCTION storage.extension(name text)
RETURNS text LANGUAGE sql AS
$$ SELECT reverse(split_part(reverse(name), '.', 1)) $$;

-- Enable RLS
ALTER TABLE storage.objects ENABLE ROW LEVEL SECURITY;
ALTER TABLE storage.buckets ENABLE ROW LEVEL SECURITY;
