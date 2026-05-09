-- Add priority scoring columns to jobs.jobs
--
-- Stores the output of the deterministic job_priority_scorer engine so that
-- the sync pipeline can set priority at ingest-time and the UI / batch
-- backfill can query without re-scoring every job every time.

-- 1. Add columns to jobs.jobs
alter table jobs.jobs
add column if not exists priority_score numeric(5,1);

alter table jobs.jobs
add column if not exists priority_tier text;

alter table jobs.jobs
add column if not exists priority_version text;

alter table jobs.jobs
add column if not exists priority_signals jsonb;

alter table jobs.jobs
add column if not exists priority_scored_at timestamptz;

-- Validate priority_tier values
alter table jobs.jobs
add constraint jobs_jobs_priority_tier_check
check (priority_tier in ('high', 'medium', 'low', 'reject'));

create index if not exists jobs_jobs_priority_score_idx on jobs.jobs (priority_score desc);
create index if not exists jobs_jobs_priority_tier_idx on jobs.jobs (priority_tier);
create index if not exists jobs_jobs_priority_scored_at_idx on jobs.jobs (priority_scored_at desc);

-- 2. Drop old upsert_job RPCs and recreate with priority params
drop function if exists jobs.upsert_job cascade;

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
  p_raw_hash text,
  p_url text default null,
  p_url_hash text default null,
  p_source_channel text default 'unknown',
  p_apply_type text default 'unknown',
  p_priority_score numeric default null,
  p_priority_tier text default null,
  p_priority_version text default null,
  p_priority_signals jsonb default null
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
    url,
    url_hash,
    source_channel,
    apply_type,
    priority_score,
    priority_tier,
    priority_version,
    priority_signals,
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
    nullif(p_url, ''),
    nullif(p_url_hash, ''),
    nullif(p_source_channel, ''),
    nullif(p_apply_type, ''),
    case when p_priority_score is not null then p_priority_score else null end,
    nullif(p_priority_tier, ''),
    nullif(p_priority_version, ''),
    p_priority_signals,
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
    url = coalesce(nullif(excluded.url, ''), jobs.jobs.url),
    url_hash = coalesce(nullif(excluded.url_hash, ''), jobs.jobs.url_hash),
    source_channel = coalesce(nullif(excluded.source_channel, ''), jobs.jobs.source_channel),
    apply_type = coalesce(nullif(excluded.apply_type, ''), jobs.jobs.apply_type),
    priority_score = case when excluded.priority_score is not null then excluded.priority_score else jobs.jobs.priority_score end,
    priority_tier = case when excluded.priority_tier is not null then excluded.priority_tier else jobs.jobs.priority_tier end,
    priority_version = case when excluded.priority_version is not null then excluded.priority_version else jobs.jobs.priority_version end,
    priority_signals = case when excluded.priority_signals is not null then excluded.priority_signals else jobs.jobs.priority_signals end,
    priority_scored_at = case
      when excluded.priority_score is not null then now()
      else jobs.jobs.priority_scored_at
    end,
    last_seen_at = now(),
    ingest_count = jobs.jobs.ingest_count + 1,
    updated_at = now()
  returning id into v_id;

  -- Also set priority_scored_at on INSERT when priority_score was provided
  if v_id is not null and p_priority_score is not null then
    update jobs.jobs set priority_scored_at = now() where id = v_id;
  end if;

  return v_id;
end;
$$;

-- 3. Recreate public wrapper
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
  p_raw_hash text,
  p_url text default null,
  p_url_hash text default null,
  p_source_channel text default 'unknown',
  p_apply_type text default 'unknown',
  p_priority_score numeric default null,
  p_priority_tier text default null,
  p_priority_version text default null,
  p_priority_signals jsonb default null
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
    p_raw_hash,
    p_url,
    p_url_hash,
    p_source_channel,
    p_apply_type,
    p_priority_score,
    p_priority_tier,
    p_priority_version,
    p_priority_signals
  );
$$;

grant execute on function public.upsert_job(
  text, text, text, text, text, text, text, text, text, text, text, jsonb, text,
  text, text, text, text, numeric, text, text, jsonb
) to anon, authenticated;
