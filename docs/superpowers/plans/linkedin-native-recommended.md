# LinkedIn Native Recommended Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Add a Rust-native `linkedin recommended` command that uses the logged-in browser session's real LinkedIn network responses to extract recommended jobs into the required JSON schema.

**Architecture:** Keep the public CLI shape as `autocli linkedin recommended --limit 0 -f json`. Register a native `CliCommand.func` for `linkedin recommended` after bundled YAML discovery and before user adapter discovery, so native behavior overrides the bundled YAML while user adapters remain able to override it. The native command drives the existing browser page, installs its own in-page response capture, uses captured LinkedIn requests/responses as the source of truth, parses records in Rust, and only derives follow-up requests from URLs/signatures observed in that same logged-in browser session.

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

Plan consequence: do not rely on CDP response-body support or `get_network_requests()` for data extraction. Implement a command-local capture script via `page.evaluate()` that stores `{ url, method, status, requestBody, responseText, responseJson }`, then read that raw capture back with `page.evaluate()`.

## Corrections From Prior Plan

- No user-agent rotation on one authenticated LinkedIn session.
- No hard-coded or guessed GraphQL endpoints; derive all URLs from captured requests.
- No CDP-level response body extraction (not supported); use in-page JS capture instead.
- Count mismatch is non-fatal by default, strict only with `--strict-count true`.

---

## Implementation Steps

- [ ] **Step 1: Register native command**

  In `crates/autocli-cli/src/commands/linkedin.rs`, implement a `CliCommand` with:
  - `name`: `"recommended"`
  - `parent`: `"linkedin"`
  - Registration after bundled YAML discovery, before user adapter discovery.
  - Flags: `--limit <n>` (default 0 = unlimited), `--strict-count <bool>` (default false), `-f json`.

- [ ] **Step 2: Implement in-page capture script**

  - Inject a JS capture via `page.evaluate()` that monkey-patches `fetch` and `XMLHttpRequest` to record `{ url, method, status, requestBody, responseText, responseJson }`.
  - Store captured entries in `window.__autocli_captured__`.
  - Trigger capture on LinkedIn's `/voyager/api/graphql` endpoints that return job recommendation data.

- [ ] **Step 3: Implement page navigation and capture flow**

  - Navigate to `https://www.linkedin.com/jobs/collections/recommended/` via `page.goto()`.
  - Wait for the job list to render.
  - Install capture script before any API calls fire.
  - Scroll to trigger lazy-loaded content.
  - Read captured responses back via `page.evaluate("window.__autocli_captured__")`.

- [ ] **Step 4: Parse captured responses into output schema**

  Parse the captured GraphQL responses in Rust, extracting for each job:
  - `job_title`
  - `company_name`
  - `location`
  - `salary`
  - `post_time`
  - `job_description`
  - `apply url`

- [ ] **Step 5: Implement pagination from observed signatures**

  - Derive pagination cursors/offsets from the captured request signatures.
  - Issue follow-up requests only using URL patterns and parameters observed in the real browser session.
  - Respect `--limit` to stop early.

- [ ] **Step 6: Handle count mismatch**

  - Compare extracted job count against LinkedIn's displayed count.
  - If mismatch: warn on stderr (default) or error out (`--strict-count true`).
  - Never include count metadata in JSON output.

- [ ] **Step 7: Output formatting**

  - Serialize parsed jobs as a JSON array to stdout.
  - Support `-f json` explicitly; JSON is the default/only format for this command.

---

## Verification

- [ ] **Step 1: Exploration crawl**

  Run:
  ```bash
  AUTOCLI_BROWSER_COMMAND_TIMEOUT=600 cargo run -p autocli-cli -- linkedin recommended --limit 3 -f json 2>output/linkedin_recommended_exploration.json
  ```

  Expected:
  - Browser launches, navigates to LinkedIn recommended jobs.
  - Capture script installs and records real API responses.
  - Output is valid JSON with up to 3 job entries.
  - If empty, inspect captured raw responses or page context to debug capture.

- [ ] **Step 2: Inspect captured responses**

  If output is empty or incomplete, inspect the raw captured data:
  ```bash
  cat output/linkedin_recommended_exploration.json | python3 -m json.tool
  ```

  Expected: enough raw JSON job responses or enough page context to debug capture.
  If not, revise capture trigger logic rather than guessing endpoints.

- [ ] **Step 3: Run full command**

  Run:
  ```bash
  mkdir -p output
  AUTOCLI_BROWSER_COMMAND_TIMEOUT=1200 cargo run -p autocli-cli -- linkedin recommended --limit 0 -f json > output/jd_full.json
  ```

  Expected:
  - stdout is valid JSON array.
  - Any count mismatch appears on stderr as a warning, not inside JSON.

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
  cargo test -p autocli-cli linkedin
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
- The plan verifies current browser/CDP support and avoids relying on unavailable CDP response-body APIs.
- The plan avoids user-agent rotation on one authenticated session.
- The plan allows pagination only from observed captured request signatures.
- Count mismatch is non-fatal by default and strict only when `--strict-count true`.
- The plan never commits `output/jd_full.json` or live LinkedIn evidence.
- The output remains a JSON array with the requested keys: `job_title`, `company_name`, `location`, `salary`, `post_time`, `job_description`, and `apply url`.
