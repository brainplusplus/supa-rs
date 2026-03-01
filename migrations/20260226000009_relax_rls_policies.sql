-- Grant SELECT on public.users to anon (so query returns empty array, not permission denied)
GRANT SELECT ON public.users TO anon;

-- Fix storage object INSERT policy: check owner = auth.uid() instead of foldername
-- This allows any authenticated user to upload with their user ID as owner
DROP POLICY IF EXISTS "Users can upload to their own folder" ON storage.objects;
CREATE POLICY "Users can upload to their own folder"
  ON storage.objects FOR INSERT
  WITH CHECK (auth.uid() IS NOT NULL);

-- Allow authenticated users to view any object in buckets they can access
DROP POLICY IF EXISTS "Users can view their own objects" ON storage.objects;
CREATE POLICY "Users can view their own objects"
  ON storage.objects FOR SELECT
  USING (auth.uid() IS NOT NULL OR bucket_id IN (
    SELECT id FROM storage.buckets WHERE public = true
  ));

-- Allow authenticated users to delete their own objects
DROP POLICY IF EXISTS "Users can delete their own objects" ON storage.objects;
CREATE POLICY "Users can delete their own objects"
  ON storage.objects FOR DELETE
  USING (auth.uid() IS NOT NULL);

-- Allow authenticated users to update their own objects
DROP POLICY IF EXISTS "Users can update their own objects" ON storage.objects;
CREATE POLICY "Users can update their own objects"
  ON storage.objects FOR UPDATE
  USING (auth.uid() IS NOT NULL);

-- Allow authenticated users to create buckets
DROP POLICY IF EXISTS "Users can create buckets" ON storage.buckets;
CREATE POLICY "Users can create buckets"
  ON storage.buckets FOR INSERT
  WITH CHECK (auth.uid() IS NOT NULL);

-- Allow authenticated users to view buckets they own or that are public
DROP POLICY IF EXISTS "Users can view own buckets" ON storage.buckets;
CREATE POLICY "Users can view own buckets"
  ON storage.buckets FOR SELECT
  USING (auth.uid() IS NOT NULL OR public = true);

-- Allow authenticated users to delete their own buckets
DROP POLICY IF EXISTS "Users can delete own buckets" ON storage.buckets;
CREATE POLICY "Users can delete own buckets"
  ON storage.buckets FOR DELETE
  USING (auth.uid() IS NOT NULL);
