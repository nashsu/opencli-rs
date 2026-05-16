-- Clean up old LinkedIn job records imported before the pipeline fix.
--
-- Invariants enforced:
--   source = 'linkedin' -> source_channel = 'recommended'
--   url is populated from raw_record->>'url' (or source_url/linkedin_url/job_url)
--   apply_type derived from raw_record->>'easy_apply'
--   apply_url set from external_url for external jobs, NULL for easy_apply
--   url_hash generated for records that now have a url

-- Step 1: Fix source_channel for old records
update jobs.jobs
set source_channel = 'recommended'
where source = 'linkedin'
  and source_channel = 'unknown';

-- Step 2: Populate url from raw_record when missing
-- Raw LinkedIn records stored the job URL in the 'url' key
update jobs.jobs
set url = coalesce(
    nullif(raw_record->>'source_url', ''),
    nullif(raw_record->>'linkedin_url', ''),
    nullif(raw_record->>'job_url', ''),
    nullif(raw_record->>'url', '')
  )
where source = 'linkedin'
  and (url is null or url = '')
  and raw_record is not null
  and raw_record != '{}'::jsonb;

-- Step 3: Generate url_hash for records that now have a url
update jobs.jobs
set url_hash = encode(sha256(coalesce(nullif(url, ''), '')::bytea), 'hex')
where source = 'linkedin'
  and (url_hash is null or url_hash = '')
  and url is not null
  and url != '';

-- Step 4: Set apply_type and apply_url based on raw_record->>'easy_apply'
-- easy_apply=true -> apply_type=easy_apply, apply_url=NULL
update jobs.jobs
set apply_type = 'easy_apply',
    apply_url = null
where source = 'linkedin'
  and apply_type = 'unknown'
  and raw_record->>'easy_apply' in ('true', 'True', '1');

-- easy_apply=false -> apply_type=external, apply_url=external_url
update jobs.jobs
set apply_type = 'external',
    apply_url = coalesce(nullif(raw_record->>'external_url', ''), external_url)
where source = 'linkedin'
  and apply_type = 'unknown'
  and raw_record->>'easy_apply' in ('false', 'False', '0');

-- Records without easy_apply in raw_record but with external_url -> apply_type=external
update jobs.jobs
set apply_type = 'external',
    apply_url = coalesce(nullif(raw_record->>'external_url', ''), external_url)
where source = 'linkedin'
  and apply_type = 'unknown'
  and (raw_record->>'easy_apply' is null or raw_record->>'easy_apply' = '')
  and nullif(raw_record->>'external_url', '') is not null;

-- Remaining records without easy_apply and without external_url -> easy_apply (LinkedIn default)
update jobs.jobs
set apply_type = 'easy_apply',
    apply_url = null
where source = 'linkedin'
  and apply_type = 'unknown'
  and (raw_record->>'easy_apply' is null or raw_record->>'easy_apply' = '')
  and nullif(raw_record->>'external_url', '') is null;
