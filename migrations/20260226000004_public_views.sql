-- public.users: runs as view owner (postgres) — no security_invoker.
-- WHERE clause enforces per-role visibility:
--   authenticated → sees only their own row
--   anon          → sees 0 rows (auth.uid() is null)
--   service_role  → sees all rows (bypasses via role claim check)
DROP VIEW IF EXISTS public.users;
CREATE OR REPLACE VIEW public.users AS
  SELECT id, email, raw_user_meta_data, created_at
  FROM auth.users
  WHERE id = auth.uid()
     OR nullif(current_setting('request.jwt.claims', true), '')::jsonb->>'role' = 'service_role';

GRANT SELECT ON public.users TO authenticated, anon, service_role;
