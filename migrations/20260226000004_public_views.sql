CREATE OR REPLACE VIEW public.users AS
  SELECT id, email, raw_user_meta_data, created_at
  FROM auth.users;

GRANT SELECT ON public.users TO authenticated;
