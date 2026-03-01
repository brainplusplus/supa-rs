-- Final consolidated RLS policies for storage.objects and storage.buckets.
-- Supersedes 005_default_rls_policies.sql, 006_storage_rls_policies.sql,
-- and 009_relax_rls_policies.sql.

-- ── storage.objects ───────────────────────────────────────────────────────────
DROP POLICY IF EXISTS "Users can upload to their own folder"  ON storage.objects;
CREATE POLICY "Users can upload to their own folder"
  ON storage.objects FOR INSERT
  WITH CHECK (auth.uid() IS NOT NULL);

DROP POLICY IF EXISTS "Users can view their own objects"      ON storage.objects;
CREATE POLICY "Users can view their own objects"
  ON storage.objects FOR SELECT
  USING (auth.uid() IS NOT NULL OR bucket_id IN (
    SELECT id FROM storage.buckets WHERE public = true
  ));

DROP POLICY IF EXISTS "Users can update their own objects"    ON storage.objects;
CREATE POLICY "Users can update their own objects"
  ON storage.objects FOR UPDATE
  USING (auth.uid() IS NOT NULL);

DROP POLICY IF EXISTS "Users can delete their own objects"    ON storage.objects;
CREATE POLICY "Users can delete their own objects"
  ON storage.objects FOR DELETE
  USING (auth.uid() IS NOT NULL);

-- ── storage.buckets ───────────────────────────────────────────────────────────
DROP POLICY IF EXISTS "Users can create buckets"              ON storage.buckets;
CREATE POLICY "Users can create buckets"
  ON storage.buckets FOR INSERT
  WITH CHECK (auth.uid() IS NOT NULL);

DROP POLICY IF EXISTS "Users can view own buckets"            ON storage.buckets;
CREATE POLICY "Users can view own buckets"
  ON storage.buckets FOR SELECT
  USING (auth.uid() IS NOT NULL OR public = true);

DROP POLICY IF EXISTS "Users can delete own buckets"          ON storage.buckets;
CREATE POLICY "Users can delete own buckets"
  ON storage.buckets FOR DELETE
  USING (auth.uid() IS NOT NULL);

-- ── auth.users ────────────────────────────────────────────────────────────────
ALTER TABLE auth.users ENABLE ROW LEVEL SECURITY;

DROP POLICY IF EXISTS "Users can see own row"                 ON auth.users;
CREATE POLICY "Users can see own row"
  ON auth.users FOR SELECT
  USING (auth.uid() = id);
