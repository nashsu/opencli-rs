-- Fix data-loss bug in jobs.upsert_job introduced by
-- 20260509182000_add_priority_scoring_columns.sql.
--
-- The INSERT body coerces NULL p_priority_score to 0 (line 113 of the
-- original migration). That means excluded.priority_score is NEVER NULL
-- inside the ON CONFLICT DO UPDATE branch — it's either the caller's
-- value or 0.
--
-- The original UPDATE branch reads:
--   priority_score = case
--     when excluded.priority_score is not null then excluded.priority_score
--     else jobs.jobs.priority_score
--   end
-- Because the case condition is ALWAYS TRUE, every unscored re-upsert
-- overwrites the existing priority_score with 0. Production already lost
-- priority history this way for any row that was re-ingested without a
-- p_priority_score on the second call.
--
-- Fix: branch on the function PARAMETER p_priority_score (which IS nullable
-- by design) instead of the excluded row. Same correction applied to
-- priority_tier, priority_scorer_version, priority_signals, priority_scored_at.
--
-- Signature is unchanged so the existing public.upsert_job wrapper and all
-- callers continue to work without modification.

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
  p_priority_scorer_version text default null,
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
    priority_scorer_version,
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
    case when p_priority_score is not null then p_priority_score else 0 end,
    coalesce(nullif(p_priority_tier, ''), 'unknown'),
    coalesce(nullif(p_priority_scorer_version, ''), 'job-priority-v1'),
    coalesce(p_priority_signals, '{}'::jsonb),
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
    -- THE FIX: branch on the function parameter (which is honestly nullable),
    -- NOT on excluded (which the INSERT body has already coerced to non-null).
    priority_score = case
      when p_priority_score is not null then p_priority_score
      else jobs.jobs.priority_score
    end,
    priority_tier = case
      when p_priority_tier is not null and p_priority_tier <> '' then p_priority_tier
      else jobs.jobs.priority_tier
    end,
    priority_scorer_version = case
      when p_priority_scorer_version is not null and p_priority_scorer_version <> '' then p_priority_scorer_version
      else jobs.jobs.priority_scorer_version
    end,
    priority_signals = case
      when p_priority_signals is not null then p_priority_signals
      else jobs.jobs.priority_signals
    end,
    priority_scored_at = case
      when p_priority_score is not null then now()
      else jobs.jobs.priority_scored_at
    end,
    last_seen_at = now(),
    ingest_count = jobs.jobs.ingest_count + 1,
    updated_at = now()
  returning id into v_id;

  -- Also set priority_scored_at on INSERT when priority_score was provided
  -- (the INSERT-side default writes 0 with NULL scored_at, so a successful
  -- explicit score needs its scored_at marker too).
  if v_id is not null and p_priority_score is not null then
    update jobs.jobs set priority_scored_at = now() where id = v_id;
  end if;

  return v_id;
end;
$$;
