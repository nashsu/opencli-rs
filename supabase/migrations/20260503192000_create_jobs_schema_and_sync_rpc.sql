-- Create jobs schema and tables for raw job ingestion + future structured extraction.
-- Also adds an RPC helper for conditional upsert behavior used by sync scripts.

create extension if not exists pgcrypto;

create schema if not exists jobs;

create table if not exists jobs.jobs (
  id uuid primary key default gen_random_uuid(),
  source text not null,
  identity_hash text not null,

  job_title text,
  company_name text,
  location text,
  salary text,
  post_time text,
  apply_url text,
  external_url text,
  job_description text,

  description_hash text,
  raw_record jsonb not null,
  raw_hash text not null,

  first_seen_at timestamptz not null default now(),
  last_seen_at timestamptz not null default now(),
  ingest_count integer not null default 1,

  created_at timestamptz not null default now(),
  updated_at timestamptz not null default now(),

  constraint jobs_source_identity_uniq unique (source, identity_hash)
);

create index if not exists jobs_jobs_company_name_idx on jobs.jobs (company_name);
create index if not exists jobs_jobs_location_idx on jobs.jobs (location);
create index if not exists jobs_jobs_last_seen_at_idx on jobs.jobs (last_seen_at desc);
create index if not exists jobs_jobs_created_at_idx on jobs.jobs (created_at desc);
create index if not exists jobs_jobs_raw_record_gin on jobs.jobs using gin (raw_record);

create table if not exists jobs.jd_structured (
  id uuid primary key default gen_random_uuid(),
  job_id uuid not null references jobs.jobs (id) on delete cascade,

  schema_version text not null,
  extractor_version text not null,
  prompt_version text not null,
  status text not null default 'pending',

  structured jsonb,
  confidence numeric,
  validation_errors jsonb,

  created_at timestamptz not null default now(),
  updated_at timestamptz not null default now(),

  constraint jd_structured_job_id_uniq unique (job_id),
  constraint jd_structured_status_check check (
    status in ('pending', 'processing', 'ok', 'failed', 'dead_letter')
  )
);

create index if not exists jd_structured_status_idx on jobs.jd_structured (status);
create index if not exists jd_structured_structured_gin on jobs.jd_structured using gin (structured);

-- RPC: upsert a job row with "do not overwrite with empty" semantics.
-- Returns the job id (uuid) of the inserted/updated row.
create or replace function jobs.upsert_job(
  p_source text,
  p_identity_hash text,
  p_job_title text,
  p_company_name text,
  p_location text,
  p_salary text,
  p_post_time text,
  p_apply_url text,
  p_external_url text,
  p_job_description text,
  p_description_hash text,
  p_raw_record jsonb,
  p_raw_hash text
)
returns uuid
language plpgsql
security definer
as $$
declare
  v_id uuid;
begin
  insert into jobs.jobs (
    source,
    identity_hash,
    job_title,
    company_name,
    location,
    salary,
    post_time,
    apply_url,
    external_url,
    job_description,
    description_hash,
    raw_record,
    raw_hash,
    first_seen_at,
    last_seen_at,
    ingest_count,
    created_at,
    updated_at
  )
  values (
    p_source,
    p_identity_hash,
    nullif(p_job_title, ''),
    nullif(p_company_name, ''),
    nullif(p_location, ''),
    nullif(p_salary, ''),
    nullif(p_post_time, ''),
    nullif(p_apply_url, ''),
    nullif(p_external_url, ''),
    nullif(p_job_description, ''),
    nullif(p_description_hash, ''),
    coalesce(p_raw_record, '{}'::jsonb),
    p_raw_hash,
    now(),
    now(),
    1,
    now(),
    now()
  )
  on conflict (source, identity_hash)
  do update set
    job_title = coalesce(nullif(excluded.job_title, ''), jobs.jobs.job_title),
    company_name = coalesce(nullif(excluded.company_name, ''), jobs.jobs.company_name),
    location = coalesce(nullif(excluded.location, ''), jobs.jobs.location),
    salary = coalesce(nullif(excluded.salary, ''), jobs.jobs.salary),
    post_time = coalesce(nullif(excluded.post_time, ''), jobs.jobs.post_time),
    apply_url = coalesce(nullif(excluded.apply_url, ''), jobs.jobs.apply_url),
    external_url = coalesce(nullif(excluded.external_url, ''), jobs.jobs.external_url),
    job_description = coalesce(nullif(excluded.job_description, ''), jobs.jobs.job_description),
    description_hash = coalesce(nullif(excluded.description_hash, ''), jobs.jobs.description_hash),
    raw_record = excluded.raw_record,
    raw_hash = excluded.raw_hash,
    last_seen_at = now(),
    ingest_count = jobs.jobs.ingest_count + 1,
    updated_at = now()
  returning id into v_id;

  return v_id;
end;
$$;

-- Public wrapper to ensure PostgREST/Supabase RPC exposure works even when the
-- `jobs` schema is not part of the exposed API schemas.
create or replace function public.upsert_job(
  p_source text,
  p_identity_hash text,
  p_job_title text,
  p_company_name text,
  p_location text,
  p_salary text,
  p_post_time text,
  p_apply_url text,
  p_external_url text,
  p_job_description text,
  p_description_hash text,
  p_raw_record jsonb,
  p_raw_hash text
)
returns uuid
language sql
security definer
as $$
  select jobs.upsert_job(
    p_source,
    p_identity_hash,
    p_job_title,
    p_company_name,
    p_location,
    p_salary,
    p_post_time,
    p_apply_url,
    p_external_url,
    p_job_description,
    p_description_hash,
    p_raw_record,
    p_raw_hash
  );
$$;

-- Allow calling the public RPC from PostgREST clients.
grant execute on function public.upsert_job(
  text, text, text, text, text, text, text, text, text, text, text, jsonb, text
) to anon, authenticated;

-- Convenience views in `public` so the Supabase Table Editor (default schema=public)
-- can show the data without switching schemas.
create or replace view public.jobs_jobs as
select * from jobs.jobs;

create or replace view public.jobs_jd_structured as
select * from jobs.jd_structured;

grant select on public.jobs_jobs to anon, authenticated;
grant select on public.jobs_jd_structured to anon, authenticated;
