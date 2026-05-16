-- Add url / url_hash columns for LinkedIn job dedup by normalized URL.
-- url_hash = sha256(normalized_url) where normalized_url has tracking params removed.
-- The unique index on url_hash acts as the DB-level constraint for dedup.

alter table jobs.jobs add column if not exists url text;
alter table jobs.jobs add column if not exists url_hash text;

create unique index if not exists jobs_jobs_url_hash_uidx on jobs.jobs (url_hash);

-- Updated RPC: accepts p_url and p_url_hash.
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
  p_url_hash text default null
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
    last_seen_at = now(),
    ingest_count = jobs.jobs.ingest_count + 1,
    updated_at = now()
  returning id into v_id;

  return v_id;
end;
$$;

-- Recreate public wrapper.
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
  p_url_hash text default null
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
    p_url_hash
  );
$$;

grant execute on function public.upsert_job(
  text, text, text, text, text, text, text, text, text, text, text, jsonb, text, text, text
) to anon, authenticated;
