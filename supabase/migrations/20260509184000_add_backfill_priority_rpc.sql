-- Add update_job_priority_score RPC for backfill scripts.
--
-- Unlike upsert_job (which handles the full row), this RPC only touches the
-- priority-scoring columns so that batch-backfill does not accidentally
-- overwrite extracted job fields that may have been enriched since ingest.

-- 1. Schema-scoped RPC
create or replace function jobs.update_job_priority_score(
  p_job_id uuid,
  p_priority_score numeric,
  p_priority_tier text,
  p_priority_scorer_version text,
  p_priority_signals jsonb
)
returns void
language plpgsql
security definer
as $$
begin
  update jobs.jobs
  set
    priority_score = p_priority_score,
    priority_tier = p_priority_tier,
    priority_scorer_version = p_priority_scorer_version,
    priority_signals = p_priority_signals,
    priority_scored_at = now(),
    updated_at = now()
  where id = p_job_id;
end;
$$;

-- 2. Public wrapper
create or replace function public.update_job_priority_score(
  p_job_id uuid,
  p_priority_score numeric,
  p_priority_tier text,
  p_priority_scorer_version text,
  p_priority_signals jsonb
)
returns void
language sql
security definer
as $$
  select jobs.update_job_priority_score(
    p_job_id,
    p_priority_score,
    p_priority_tier,
    p_priority_scorer_version,
    p_priority_signals
  );
$$;

grant execute on function public.update_job_priority_score(
  uuid, numeric, text, text, jsonb
) to anon, authenticated;
