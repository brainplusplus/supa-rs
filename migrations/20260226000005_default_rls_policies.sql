-- Storage: authenticated users dapat akses bucket public
DROP POLICY IF EXISTS "Public buckets are viewable by everyone" ON storage.buckets;
CREATE POLICY "Public buckets are viewable by everyone"
  ON storage.buckets FOR SELECT
  USING (public = true);

DROP POLICY IF EXISTS "Users can upload to their own folder" ON storage.objects;
CREATE POLICY "Users can upload to their own folder"
  ON storage.objects FOR INSERT
  WITH CHECK (auth.uid()::text = (storage.foldername(name))[1]);

DROP POLICY IF EXISTS "Users can view their own objects" ON storage.objects;
CREATE POLICY "Users can view their own objects"
  ON storage.objects FOR SELECT
  USING (auth.uid() = owner);

DROP POLICY IF EXISTS "Users can delete their own objects" ON storage.objects;
CREATE POLICY "Users can delete their own objects"
  ON storage.objects FOR DELETE
  USING (auth.uid() = owner);
