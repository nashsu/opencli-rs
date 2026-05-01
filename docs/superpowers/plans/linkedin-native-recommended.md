# LinkedIn Native Recommended Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Add a Rust-native `linkedin recommended` command that uses the logged-in browser session's real LinkedIn network responses to extract recommended jobs into the required JSON schema.

**Architecture:** Keep the public CLI shape as `autocli linkedin recommended --limit 0 -f json`. Register a native `CliCommand.func` for `linkedin recommended` after bundled YAML discovery and before user adapter discovery, so native behavior overrides the bundled YAML while user adapters remain able to override it. The native command drives the existing browser page, installs its own in-page response capture after navigation (so the patch survives the page lifecycle), uses captured LinkedIn requests/responses as the source of truth, parses records in Rust, captures both list and detail responses keyed by `job_id`, preserves full request signatures for replay, and only paginates when the observed URL or body can be safely transformed.

**Tech Stack:** Rust 2021, `autocli-core::CliCommand`, `autocli-core::IPage`, AutoCLI `BrowserBridge`/daemon page, browser `fetch`/XHR capture through `IPage::evaluate`, `tokio`, `serde_json`, current output renderer.

---

## Verified Browser/CDP Support

Serena was used to verify existing browser support before this revision:

- `IPage` supports `goto`, `evaluate`, `cookies`, `snapshot`, `auto_scroll`, `intercept_requests`, `get_intercepted_requests`, and `get_network_requests`.
- `DaemonPage::cookies` returns browser cookies through the daemon.
- `DaemonPage::evaluate` executes JavaScript in the logged-in browser page.
- `DaemonPage::intercept_requests` and `CdpPage::intercept_requests` install JS monkey-patches for fetch/XHR.
- `get_network_requests()` is Performance API metadata only; it does not provide response bodies.
- Existing typed `get_intercepted_requests()` can lose arbitrary raw JSON response bodies because it deserializes into `InterceptedRequest`.

Plan consequence: do not rely on CDP response-body support or `get_network_requests()` for data extraction. Implement a command-local capture script via `page.evaluate()` that stores `{ url, method, status, requestHeaders, requestBody, responseText, responseJson }`, then read that raw capture back with `page.evaluate()`.

## Corrections From Prior Plan

- No user-agent rotation on one authenticated LinkedIn session.
- No hard-coded or guessed GraphQL endpoints; derive all URLs from captured requests.
- No CDP-level response body extraction (not supported); use in-page JS capture instead.
- Count mismatch is non-fatal by default, strict only with `--strict-count true`.

## MVP Rework — Gaps Fixed

1. **Capture survives navigation:** The original plan injected capture before `goto()`, so the JS patch was destroyed when the page navigated. Revised: navigate first, wait for the page to render, then install the in-page capture so it lives for the lifetime of the page. If `intercept_requests` is available as a persistent CDP-level hook, prefer that as an additional safety net.

2. **Detail responses are captured and merged by `job_id`:** The original plan only parsed list/card GraphQL responses, so `job_description` was always `N/A`. Revised: after extracting job IDs from list responses, trigger per-job detail fetches (by clicking cards or navigating to detail endpoints), capture those detail responses, and merge `job_description` back into each record by `job_id`.

3. **Full request signature is preserved:** The original capture schema dropped `requestHeaders`, `method`, and `requestBody`. Revised: the capture store includes `{ url, method, status, requestHeaders, requestBody, responseText, responseJson }` so follow-up requests can replay the exact signature observed in the browser session.

4. **Pagination is gated on safe transformability:** The original plan blindly replaced `start:0` in the URL. LinkedIn may use non-zero start values, base64-encoded variables in the request body, or cursor-based pagination. Revised: inspect the captured list request. If pagination uses a simple integer `start` query parameter, increment it by the page size. If pagination uses a JSON body with a `start` field, transform the body. If the mechanism is opaque (encoded variables, opaque cursors), stop pagination and emit a warning to stderr explaining why. Never fabricate pagination parameters.

5. **Test commands use valid filter syntax:** `cargo test -p autocli-cli linkedin` passes two test name filters, which Cargo rejects. Revised verification steps use `cargo test -p autocli-cli -- linkedin` (module-scoped) or `cargo test -p autocli-cli linkedin::tests` (exact path).

---

## Implementation Steps

- [ ] **Step 1: Register native command**

  In `crates/autocli-cli/src/commands/linkedin.rs`, implement a `CliCommand` with:
  - `name`: `"recommended"`
  - `parent`: `"linkedin"`
  - Registration after bundled YAML discovery, before user adapter discovery.
  - Flags: `--limit <n>` (default 0 = unlimited), `--strict-count <bool>` (default false), `-f json`.

- [ ] **Step 2: Implement in-page capture script (post-navigation)**

  Design a JS capture script that:
  - Is injected via `page.evaluate()` **after** `page.goto()` completes and the job list renders.
  - Monkey-patches `fetch` and `XMLHttpRequest` to record the full signature:
    `{ url, method, status, requestHeaders, requestBody, responseText, responseJson }`
  - Stores captured entries in `window.__autocli_captured__`.
  - Captures all `/voyager/api/graphql` (or equivalent) requests — both list queries and detail queries.
  - Survives for the lifetime of the page (no navigation after injection).
  - If `page.intercept_requests()` provides a persistent CDP-level hook, use it as a secondary capture layer to catch requests that fire before the inline script activates.

- [ ] **Step 3: Implement page navigation and list capture flow**

  - Navigate to `https://www.linkedin.com/jobs/collections/recommended/` via `page.goto()`.
  - Wait for the job list DOM to render (poll for card elements or a known container selector).
  - **Then** install the capture script from Step 2.
  - Scroll incrementally to trigger lazy-loaded list API calls.
  - Read captured list responses back via `page.evaluate("window.__autocli_captured__")`.
  - Parse list responses in Rust to extract: `job_id`, `job_title`, `company_name`, `location`, `salary`, `post_time`, `apply url`.

- [ ] **Step 4: Capture and merge job-detail responses**

  - From the parsed list, collect all unique `job_id` values.
  - For each `job_id`, trigger a detail fetch by either:
    - Clicking each job card in the list (which LinkedIn's UI translates to a detail API call), or
    - Navigating the browser to each job's detail URL and capturing the resulting API responses.
  - After each trigger, poll `window.__autocli_captured__` for new entries keyed by `job_id`.
  - Parse detail responses to extract `job_description`.
  - Merge `job_description` into each output record by matching `job_id`.
  - If a detail response is never captured for a given `job_id`, emit `job_description: null` and warn on stderr.

- [ ] **Step 5: Implement safe pagination**

  - Inspect the captured list request that produced the first page of results.
  - Identify the pagination mechanism:
    - **URL query parameter `start` (integer):** safe — increment by the page size.
    - **JSON request body field `start` (integer):** safe — transform the body and replay with preserved headers.
    - **Base64-encoded variables, opaque cursors, or non-integer pagination:** unsafe — stop and warn.
  - For safe mechanisms, replay the captured request with the transformed URL/body via `page.evaluate()` (using `fetch` from within the page context to reuse cookies/headers).
  - Capture and parse the new responses, merging into the result set.
  - Stop when `--limit` is reached, no more results are returned, or pagination is exhausted.
  - If pagination is unsafe, emit a stderr warning: `"pagination mechanism not transformable; returning only initial page results"`.

- [ ] **Step 6: Handle count mismatch**

  - Compare extracted job count against LinkedIn's displayed count (scraped from the page DOM).
  - If mismatch: warn on stderr (default) or error out with a non-zero exit code (`--strict-count true`).
  - Never include count metadata in JSON output.

- [ ] **Step 7: Output formatting**

  - Serialize parsed jobs as a JSON array to stdout.
  - Support `-f json` explicitly; JSON is the default/only format for this command.
  - Ensure the output schema matches: `job_title`, `company_name`, `location`, `salary`, `post_time`, `job_description`, `apply url`.

---

## Verification

- [ ] **Step 1: Exploration crawl**

  Run:
  ```bash
  AUTOCLI_BROWSER_COMMAND_TIMEOUT=600 cargo run -p autocli-cli -- linkedin recommended --limit 3 -f json 2>output/linkedin_recommended_exploration.json
  ```

  Expected:
  - Browser launches, navigates to LinkedIn recommended jobs.
  - Capture script is installed AFTER page render.
  - Output is valid JSON with up to 3 job entries, each with a non-null `job_description` (from detail responses).
  - If empty or `job_description` is null, inspect `output/linkedin_recommended_exploration.json` for raw capture diagnostics.

- [ ] **Step 2: Inspect captured responses**

  If output is empty or detail data is missing, inspect the raw captured data:
  ```bash
  cat output/linkedin_recommended_exploration.json | python3 -m json.tool
  ```

  Expected: enough raw JSON job responses or enough page context to debug capture.
  If not, check whether capture was installed after navigation and whether detail triggers fired API calls.

- [ ] **Step 3: Run full command**

  Run:
  ```bash
  mkdir -p output
  AUTOCLI_BROWSER_COMMAND_TIMEOUT=1200 cargo run -p autocli-cli -- linkedin recommended --limit 0 -f json > output/jd_full.json
  ```

  Expected:
  - stdout is valid JSON array.
  - Any count mismatch appears on stderr as a warning, not inside JSON.
  - If pagination is unsafe, a warning explains why only the initial page was returned.

- [ ] **Step 4: Validate JSON schema**

  Run:
  ```bash
  python3 - <<'PY'
  import json
  from pathlib import Path

  data = json.loads(Path("output/jd_full.json").read_text())
  required = ["job_title", "company_name", "location", "salary", "post_time", "job_description", "apply url"]
  assert isinstance(data, list), type(data)
  for index, item in enumerate(data):
      missing = [key for key in required if key not in item]
      assert not missing, (index, missing)
  print(len(data))
  PY
  ```

  Expected:
  - Prints the output array length.
  - No assertion fails.

- [ ] **Step 5: Verify strict count behavior**

  Run:
  ```bash
  AUTOCLI_BROWSER_COMMAND_TIMEOUT=1200 cargo run -p autocli-cli -- linkedin recommended --limit 0 --strict-count true -f json > output/jd_full_strict.json
  ```

  Expected:
  - Passes if output count equals displayed count.
  - Fails with a clear count mismatch error if LinkedIn drifts during crawl.

- [ ] **Step 6: Final checks**

  Run:
  ```bash
  cargo test -p autocli-cli -- linkedin
  cargo check -q
  ```

  Expected: both pass.

- [ ] **Step 7: Commit code only**

  Run:
  ```bash
  git add crates/autocli-cli/src/commands/linkedin.rs crates/autocli-cli/src/commands/mod.rs crates/autocli-cli/src/main.rs
  git commit -m "feat(linkedin): crawl recommended jobs natively"
  ```

  Do not add `output/jd_full.json`, `output/jd_full_strict.json`, or `output/linkedin_recommended_exploration.json`.

## Self-Review

- The plan uses real network responses from the logged-in browser session as the source of truth.
- Capture is installed **after** navigation so the JS patch survives the page lifecycle.
- Both list and detail responses are captured and merged by `job_id`; `job_description` is populated from real data.
- The full request signature (`url`, `method`, `status`, `requestHeaders`, `requestBody`, `responseText`, `responseJson`) is preserved for replay.
- Pagination only proceeds when the observed mechanism is safely transformable; opaque pagination stops with a warning.
- The plan verifies current browser/CDP support and avoids relying on unavailable CDP response-body APIs.
- The plan avoids user-agent rotation on one authenticated session.
- Count mismatch is non-fatal by default and strict only when `--strict-count true`.
- The plan never commits `output/jd_full.json` or live LinkedIn evidence.
- The output remains a JSON array with the requested keys: `job_title`, `company_name`, `location`, `salary`, `post_time`, `job_description`, and `apply url`.
- Test commands use valid Cargo filter syntax (`cargo test -p autocli-cli -- linkedin`).
