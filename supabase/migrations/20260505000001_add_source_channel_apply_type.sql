-- Standardize source field, add source_channel / apply_type, create job_source_records.

-- 1. Add new columns to jobs.jobs
alter table jobs.jobs
add column if not exists source_channel text not null default 'unknown';

alter table jobs.jobs
add column if not exists apply_type text not null default 'unknown';

alter table jobs.jobs
add constraint jobs_jobs_apply_type_check
check (apply_type in ('easy_apply', 'external', 'unknown'));

-- 2. Migrate existing linkedin_recommended → linkedin + recommended
update jobs.jobs
set source = 'linkedin',
    source_channel = 'recommended'
where source = 'linkedin_recommended';

-- 3. Update records where easy_apply was set in raw_record
update jobs.jobs
set apply_type = 'easy_apply'
where source = 'linkedin'
  and raw_record->>'easy_apply' in ('true', 'True');

update jobs.jobs
set apply_type = 'external'
where source = 'linkedin'
  and raw_record->>'easy_apply' in ('false', 'False');

-- 4. Drop old upsert_job RPCs and recreate with url_hash as conflict target + new fields.
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
  p_apply_type text default 'unknown'
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
    last_seen_at = now(),
    ingest_count = jobs.jobs.ingest_count + 1,
    updated_at = now()
  returning id into v_id;

  return v_id;
end;
$$;

-- 5. Update public wrapper
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
  p_apply_type text default 'unknown'
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
    p_apply_type
  );
$$;

grant execute on function public.upsert_job(
  text, text, text, text, text, text, text, text, text, text, text, jsonb, text, text, text, text, text
) to anon, authenticated;

-- 6. Create job_source_records table
create table if not exists jobs.job_source_records (
  id bigserial primary key,
  job_id uuid references jobs.jobs (id) on delete cascade,
  source text not null,
  source_channel text not null default 'unknown',
  source_job_id text,
  external_url text,
  normalized_url text,
  url_hash text not null,
  easy_apply boolean,
  raw_record jsonb,
  scraped_at timestamptz not null default now(),
  created_at timestamptz not null default now(),
  unique (source, url_hash)
);

create index if not exists job_source_records_job_id_idx on jobs.job_source_records (job_id);
create index if not exists job_source_records_url_hash_idx on jobs.job_source_records (url_hash);
create index if not exists job_source_records_scraped_at_idx on jobs.job_source_records (scraped_at desc);
