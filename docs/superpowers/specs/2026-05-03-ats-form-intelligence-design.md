# ATS Form Intelligence Design

## Goal

Build the first production foundation for ATS form intelligence in AutoCLI. The system reacts to jobs written to Supabase or an existing queue, uses each job's `external_url` as the source of truth, extracts ATS/platform/form evidence, and persists structured intelligence without submitting applications.

This is not an auto-apply system. It must never submit applications, bypass CAPTCHA, automate Google/SSO/password/MFA/passkey login, collect hidden credentials, or invent form fields without DOM/API/network evidence.

## Repository Context

AutoCLI is a Rust workspace with reusable browser and discovery primitives:

- `autocli-core::IPage` defines browser operations for navigation, JS evaluation, snapshots, screenshots, cookies, tabs, interception, and network requests.
- `autocli-browser` provides `BrowserBridge`, `DaemonPage`, and `CdpPage`.
- `autocli-ai::explore` already performs API surface discovery, JSON suffix probing, `__INITIAL_STATE__` extraction, framework detection, Pinia/Vuex store discovery, and endpoint analysis.
- `autocli-ai::generate` and `cascade` provide useful discovery patterns, but the ATS system must not expose a CLI-first workflow as the production path.
- The repo does not currently contain a durable Supabase/queue layer. The ATS design adds a worker-facing boundary for the already-existing Supabase schema.

Serena and a bounded read-only explorer were used for the repository inspection. Supabase MCP was requested, but no Supabase MCP namespace was exposed in this session, so live schema inspection is not part of this design pass.

## Chosen Approach

Use Alternative 1: one focused crate plus a minimal worker entrypoint.

Add:

- `crates/autocli-ats`: ATS core library with explicit internal modules.
- `crates/autocli-ats-worker`: minimal event-driven worker binary.

This keeps the first code change smaller than a multi-crate service split while preserving clean module boundaries. The production path is the worker, not a user-facing CLI.

## Core Modules

`core`
: Job input/output types, status machine, canonical URL/hash, ATS detection result, required output JSON schema, schema hash helpers, and safety constants.

`orchestrator`
: Idempotent job flow. It loads the job from the queue payload, canonicalizes `external_url`, checks caches, runs discovery/session/browser steps, persists terminal or blocked states, and acknowledges the queue message only after persistence succeeds.

`discovery`
: Deterministic ATS detection for Lever, Greenhouse, Ashby, SmartRecruiters, Workday, Google Careers, and Generic. It also adapts `autocli-ai::explore` into structured ATS platform evidence when browser verification is required.

`browser`
: `BrowserIntelExtractor` over `Arc<dyn IPage>`. The MVP extracts Lever and generic public forms from DOM/accessibility/network evidence. It records fields, labels, required state, file upload requirements, buttons, final submit existence, login walls, CAPTCHA markers, multi-page shape, and observed network evidence.

`session`
: Metadata-only session gate. It checks whether a valid session exists for user/platform/provider/domain, creates login requests when needed, and never automates login or stores plaintext session material.

`supabase`
: Repository and queue adapter for the existing Supabase schema. It consumes existing tables/queues, writes statuses/intelligence/login requests, and provides dedupe/lock operations. No live Supabase changes are applied by the worker.

`worker`
: Small loop that reads a bounded batch from the existing queue, calls the orchestrator, persists success/failure/blocking state, and exits or sleeps based on config.

## Supabase Schema Contract

The worker consumes the existing Supabase schema. It expects the existing system to provide tables or compatible views for:

- `jobs`
- `ats_job_form_intelligence`
- `ats_platform_intelligence`
- `ats_sessions`
- `login_requests`
- queue/dead-letter concepts equivalent to `ats_intel_requested`, `browser_intel_requested`, `alert_requested`, and `dead_letter`

The current `jobs` table was reported to be missing storage for the stashed external application URL. This branch includes a non-applied Supabase SQL contract at `docs/supabase/ats_jobs_external_url_contract.sql` for review, but the worker must not automatically apply migrations.

Minimum job fields needed by the ATS worker:

- `id`
- `company`
- `title`
- `url` or LinkedIn source URL
- `external_url`
- `external_url_hash`
- `ats_platform`
- `ats_intel_status`
- `ats_intel_id`
- `ats_intel_error`
- `ats_intel_requested_at`
- `ats_intel_completed_at`

If the live schema uses different names, `autocli-ats::supabase` should map those names at the repository boundary instead of leaking schema differences into the orchestrator.

## Event Flow

1. Worker receives an `ats_intel_requested` queue message or equivalent existing Supabase queue row.
2. Orchestrator loads the referenced job.
3. Orchestrator rejects or blocks the job if `external_url` is absent.
4. URL canonicalizer removes tracking parameters such as `utm_source`, `utm_medium`, `utm_campaign`, `lever-source`, `gh_src`, `source`, and `ref`, while preserving job-identifying parameters.
5. Detector extracts platform, domain, company slug, posting ID, job ID, or req ID when possible.
6. Job-level cache lookup checks `ats_job_form_intelligence` by canonical URL hash, posting identifiers, and schema hash where available.
7. Platform-level cache lookup checks `ats_platform_intelligence`.
8. Session gate checks platform/domain/provider session state before browser extraction when needed.
9. Browser extractor opens public pages through existing `IPage` implementations, initially `BrowserBridge`/CDP-backed.
10. Extractor records deterministic DOM/accessibility/network evidence and stops before final submit.
11. Orchestrator persists `ats_job_form_intelligence` and updates the job status.
12. Login/CAPTCHA states create login requests or alert queue entries and leave jobs requeueable.

## Status Machine

Use explicit statuses:

- `pending`
- `queued`
- `checking_cache`
- `cache_hit`
- `processing`
- `discovery_required`
- `session_checking`
- `login_required`
- `session_ready`
- `browser_extracting`
- `normalizing`
- `ok`
- `captcha_required`
- `expired_job`
- `unsupported`
- `failed`
- `dead_letter`

Login and CAPTCHA are expected blocked states, not generic failures.

## Cache And Dedupe

The orchestrator is cache-first. It opens a browser only after job-level and platform-level cache checks miss or require verification.

Suggested dedupe keys:

- `ats_intel:{canonical_apply_url_hash}`
- `ats_discovery:{ats_platform}:{domain}`
- `browser_intel:{canonical_apply_url_hash}:{session_id_or_public}`

The Supabase adapter owns lock acquisition and duplicate message handling. The orchestrator remains deterministic and idempotent.

## Browser Extraction Rules

The MVP extractor supports:

- public Lever forms
- generic public HTML forms
- login wall detection
- CAPTCHA detection
- final submit detection without clicking submit
- resume/file upload detection
- field label, placeholder, autocomplete, required, visible, disabled, and nearby text extraction
- button extraction
- single-page flow graph
- limited safe reveal actions for non-final `Start application`, `Next`, or `Continue` controls
- timeout and bounded step count

It does not submit, solve CAPTCHA, enter credentials, or automate login.

Network evidence can come from existing `IPage::get_network_requests`, `intercept_requests`, and page-level JS capture. Existing `autocli-ai::explore` can enrich platform discovery, but it must not become the source of truth for final form fields.

## LLM Boundary

LLM normalization is optional and not part of the first critical path. If added, it may classify field semantic roles or endpoint purpose from deterministic evidence only. All LLM output must be schema-validated and must not invent fields, decide to submit, bypass CAPTCHA, or produce final intelligence without evidence.

## Error Handling

Malformed queue messages, missing required job identifiers, Supabase persistence errors, and repeated infrastructure failures can go to `dead_letter` after bounded retries.

Missing login sessions produce:

- job status `login_required`
- `login_requests` row with provider/domain/login URL/reason/status
- alert queue row if the existing system supports alerts

CAPTCHA produces:

- job status `captcha_required`
- evidence with CAPTCHA type when identifiable
- no bypass attempt

Browser failures produce structured `failed` status only after capturing the last known page URL, platform, extraction step, and error.

## Tests

Add tests close to `crates/autocli-ats`.

Fixtures:

- Lever public application HTML/URL
- Greenhouse URL pattern
- Ashby URL pattern
- Workday login-required URL pattern
- Google Careers login-required URL pattern
- unknown generic form HTML
- CAPTCHA marker HTML
- expired job page HTML

Test coverage:

- URL canonicalization
- ATS detection
- cache hit path
- cache miss path
- login required path
- CAPTCHA required path
- Lever form extraction
- final submit detection without clicking submit
- output schema validation
- idempotent queue handling

Supabase tests should use in-memory fakes implementing the same repository/queue traits unless Supabase MCP or a test database is available in the implementation session.

## Implementation Deliverables

First implementation pass:

- Add `crates/autocli-ats` with modules for core, orchestrator, discovery, browser, session, and supabase boundaries.
- Add `crates/autocli-ats-worker` as a minimal worker binary.
- Add Rust tests and ATS HTML/JSON fixtures.
- Keep `docs/supabase/ats_jobs_external_url_contract.sql` as the reviewed, non-applied contract documenting the required `jobs.external_url` storage gap.
- Reuse `autocli-core::IPage`, `autocli-browser::BrowserBridge`, `autocli-browser::CdpPage`, and `autocli-ai::explore`.
- Avoid polished CLI UX. A local replay tool may be added later only for debugging.

Out of scope for the first pass:

- applying Supabase migrations automatically
- full Workday extraction
- Google Careers login automation
- CAPTCHA solving
- final submit automation
- Playwright backend
- LLM-based final schema generation

## Approval State

Approved design choices from brainstorming:

- Use one focused ATS crate plus a minimal worker binary.
- Consume the existing Supabase schema, with the latest update that the branch must document the missing job `external_url` storage gap.
- Use existing AutoCLI `IPage` with `BrowserBridge`/CDP for MVP browser extraction.
- Keep Playwright as a future backend behind the extractor trait.
- Make the worker path primary and keep CLI/replay tooling secondary.
