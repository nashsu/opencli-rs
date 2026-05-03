-- ATS Form Intelligence job external URL contract.
--
-- This file documents the Supabase jobs-table fields required by the ATS
-- worker. It is intentionally not wired into an automatic migration runner.
-- Review against the live Supabase schema before applying.

alter table public.jobs
  add column if not exists external_url text,
  add column if not exists external_url_hash text,
  add column if not exists ats_platform text,
  add column if not exists ats_intel_status text,
  add column if not exists ats_intel_id uuid,
  add column if not exists ats_intel_error text,
  add column if not exists ats_intel_requested_at timestamptz,
  add column if not exists ats_intel_completed_at timestamptz;

create index if not exists jobs_external_url_hash_idx
  on public.jobs (external_url_hash)
  where external_url_hash is not null;

create index if not exists jobs_ats_intel_status_idx
  on public.jobs (ats_intel_status)
  where ats_intel_status is not null;
