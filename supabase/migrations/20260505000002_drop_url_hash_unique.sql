-- Drop the url_hash unique constraint (replaced by (source, identity_hash) in upsert)
-- url_hash dedup is handled at the application level in clean_linkedin_jobs.py
-- A regular index is sufficient for query performance

drop index if exists jobs.jobs_jobs_url_hash_uidx;

create index if not exists jobs_jobs_url_hash_idx on jobs.jobs (url_hash)
where url_hash is not null and url_hash != '';
