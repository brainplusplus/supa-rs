-- Schema access for all roles
GRANT USAGE ON SCHEMA public  TO anon, authenticated, service_role;
GRANT USAGE ON SCHEMA storage TO anon, authenticated, service_role;
GRANT USAGE ON SCHEMA auth    TO service_role;

-- public schema
GRANT ALL    ON ALL TABLES    IN SCHEMA public  TO authenticated;
GRANT SELECT ON ALL TABLES    IN SCHEMA public  TO anon;
GRANT ALL    ON ALL TABLES    IN SCHEMA public  TO service_role;
GRANT ALL    ON ALL SEQUENCES IN SCHEMA public  TO service_role;

-- storage schema
GRANT SELECT, INSERT, UPDATE, DELETE ON ALL TABLES IN SCHEMA storage TO authenticated;
GRANT SELECT                          ON ALL TABLES IN SCHEMA storage TO anon;
GRANT ALL    ON ALL TABLES    IN SCHEMA storage TO service_role;
GRANT ALL    ON ALL SEQUENCES IN SCHEMA storage TO service_role;

-- auth schema (service_role only)
GRANT ALL ON ALL TABLES    IN SCHEMA auth TO service_role;
GRANT ALL ON ALL SEQUENCES IN SCHEMA auth TO service_role;

-- Future tables auto-covered
ALTER DEFAULT PRIVILEGES IN SCHEMA public  GRANT ALL ON TABLES    TO service_role;
ALTER DEFAULT PRIVILEGES IN SCHEMA storage GRANT ALL ON TABLES    TO service_role;
ALTER DEFAULT PRIVILEGES IN SCHEMA auth    GRANT ALL ON TABLES    TO service_role;
ALTER DEFAULT PRIVILEGES IN SCHEMA public  GRANT ALL ON SEQUENCES TO service_role;
ALTER DEFAULT PRIVILEGES IN SCHEMA storage GRANT ALL ON SEQUENCES TO service_role;
ALTER DEFAULT PRIVILEGES IN SCHEMA auth    GRANT ALL ON SEQUENCES TO service_role;
