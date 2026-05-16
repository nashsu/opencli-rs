-- Enable Row Level Security on jobs.jobs and grant a read-only policy
-- for the anon role.
--
-- Reason: the autocli-daily microservice's /jobs HTTP endpoint queries
-- this table via the Supabase anon key. The anon key is intentionally
-- public (Supabase design) — the safety boundary is RLS, not key secrecy.
-- Without RLS, the anon key gives whoever has it read/write to every row.
--
-- Combined with deploy/SPEC.md §5.3 (Cloudflare Access in front of the
-- /jobs endpoint) this gives defence in depth: Access at the edge +
-- Bearer at the app + RLS at the database. Even if the first two fail
-- open, the database itself only exposes SELECT on jobs.jobs to anon —
-- no writes, no other tables in the jobs schema.
--
-- Writes via sync_autocli_jobs.py continue to use SUPABASE_SERVICE_ROLE_KEY
-- which bypasses RLS.

alter table jobs.jobs enable row level security;

-- Anon (and authenticated) clients may read every row. We deliberately do
-- not filter by ownership because all rows are scraped public job postings
-- and the /jobs endpoint serves them as a list. Tighten this policy if
-- per-user filtering becomes a requirement.
create policy anon_read_jobs_jobs on jobs.jobs
  for select
  to anon, authenticated
  using (true);

-- No INSERT / UPDATE / DELETE policies for anon — those operations remain
-- service-role-only by virtue of RLS being enabled and no permissive
-- policies for those verbs existing.
