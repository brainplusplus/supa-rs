-- Buckets: owner bisa INSERT dan SELECT bucketnya sendiri
DROP POLICY IF EXISTS "Users can create buckets" ON storage.buckets;
CREATE POLICY "Users can create buckets"
  ON storage.buckets FOR INSERT
  WITH CHECK (owner = auth.uid());

DROP POLICY IF EXISTS "Users can view own buckets" ON storage.buckets;
CREATE POLICY "Users can view own buckets"
  ON storage.buckets FOR SELECT
  USING (owner = auth.uid() OR public = true);

DROP POLICY IF EXISTS "Users can delete own buckets" ON storage.buckets;
CREATE POLICY "Users can delete own buckets"
  ON storage.buckets FOR DELETE
  USING (owner = auth.uid());

DROP POLICY IF EXISTS "Users can update their own objects" ON storage.objects;
CREATE POLICY "Users can update their own objects"
  ON storage.objects FOR UPDATE
  USING (auth.uid() = owner);
