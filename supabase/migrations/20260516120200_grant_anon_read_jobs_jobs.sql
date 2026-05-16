-- Companion to 20260516120100: RLS by itself doesn't grant SELECT — PostgREST
-- requires both an explicit GRANT and an RLS policy that passes. Without
-- this GRANT, the /jobs endpoint returned count=0 even though the
-- anon_read_jobs_jobs policy USING(true) was active, because the anon role
-- had no SELECT privilege on the table or USAGE on the schema.
--
-- Supabase auto-grants these for tables in `public` by default; custom
-- schemas exposed via the dashboard's "Exposed schemas" setting still need
-- the GRANTs to be explicit.

grant usage on schema jobs to anon, authenticated;
grant select on jobs.jobs to anon, authenticated;
