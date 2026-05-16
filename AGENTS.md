# AGENTS.md
# Core Rule

Use Serena first for code intelligence on non-trivial coding tasks, and use bounded subagents for complex engineering work.

Do not claim Serena or subagents were used unless they actually were. If a required tool is unavailable, say so and continue with the smallest safe fallback.

## Serena Workflow

At the start of any non-trivial coding task (see definitions below), unfamiliar-code task, bug investigation, shared-symbol change, or cross-file change:

Definitions:
- **Non-trivial**: Any change affecting ≥1 function with external dependencies, ≥3 files, or requiring architectural reasoning
- **Trivial**: Typo fixes, one-line config changes, single-file docs edits without code path impact

Do not run the full Serena workflow for trivial tasks unless the code path is unfamiliar or risky.

1. Check Serena availability.
2. Run `serena.get_current_config`.
3. If the active Serena project does not match the repository root, run `serena.activate_project`.
4. Run `serena.check_onboarding_performed`.
5. If onboarding is missing, run `serena.onboarding`.
6. Read only relevant Serena memories.

If Serena is unavailable, say:

> Serena MCP is unavailable; falling back to built-in search/read tools.

Then continue with targeted `rg`, file reads, and normal verification.

Do not run the full Serena workflow for typo fixes, simple docs edits, or one-line config changes unless the code path is unfamiliar or risky.

## Serena Navigation

Prefer Serena before broad file reads:

1. `serena.get_symbols_overview` for unfamiliar files.
2. `serena.find_symbol` for functions, classes, handlers, schemas, adapters, providers, components, exported APIs, and config objects.
3. `serena.find_referencing_symbols` before changing shared/public symbols.
4. `serena.find_implementations` for interfaces, adapters, providers, and polymorphic dispatch.
5. `serena.get_diagnostics_for_file` after meaningful edits.

Use raw `rg`, grep, or full-file reads only when:

- the target is not code,
- the symbol name is unknown,
- Serena cannot resolve the result,
- Serena has already narrowed the search area,
- or the task is trivial enough that Serena overhead exceeds value.

Do not read entire large files first.

## Editing Rules

Before editing:

- Map the real call path.
- Check references for shared/exported symbols.
- Pick the smallest safe patch.
- Avoid unrelated files.
- Prefer symbol-level edits for whole functions/classes/methods.
- Add or update tests when behavior changes.

After editing:

1. Run the smallest relevant verification first (see Verification Tiers below).
2. Then run broader checks if the change is cross-file or high-risk.
3. Summarize changed files, reason, and verification result.

Verification Tiers:
- **Tier 1 (local)**: Single unit test or type check for the edited function/method
- **Tier 2 (module)**: All tests in the affected package/directory
- **Tier 3 (integration)**: Cross-module or end-to-end verification for cross-file/high-risk changes

## Subagent Policy

Use subagents for:

- cross-file or cross-module changes,
- unknown root cause,
- refactors,
- security/auth changes,
- data-loss or migration risk,
- queue/worker/scraper/infra changes,
- PR or adversarial review,
- bugs where investigation, review, and fix can be separated.

Do not use subagents for:

- direct Q&A,
- typo fixes,
- one-file trivial edits,
- simple config changes,
- tasks where overhead exceeds value.

If subagents are unavailable, say so and continue in the parent agent using the same sequence manually: explore read-only, review risks, patch only if needed, then verify.

## Subagent Roles

- `explorer`: read-only. Map execution paths, symbols, references, data flow, likely owners, and risky files.
- `reviewer`: read-only. Look for correctness bugs, regressions, race conditions, idempotency issues, auth/security problems, migration/data-loss risks, missing tests, and rollback gaps.
- `fixer`: may edit only after the code path is understood. Keep the patch small, avoid unrelated files, use Serena reference checks, and verify targeted changes.

Subagents may recommend actions, but must not broaden scope, introduce new architecture, or modify unrelated modules without parent approval.

## Subagent Flow

For complex tasks:

1. Spawn `explorer` first.
2. Spawn `reviewer` in parallel only when risk review helps.
3. Wait for read-only findings.
4. Summarize the evidence.
5. Spawn `fixer` only if a patch is needed.
6. Run verification.
7. For high-risk changes, run one final reviewer pass.

Default limit:

- `explorer`: at most 1 before editing
- `reviewer`: at most 1 in parallel with explorer or after
- `fixer`: at most 1, only after read-only findings are complete
- Do not create more subagents unless the user explicitly asks or a P0/P1 risk remains unresolved.
- Maximum total: 3 subagents per task (2 read-only + 1 fixer)

Subagents must return:

- scope inspected,
- Serena tools used,
- key symbols/files,
- findings,
- risks,
- recommended next action,
- confidence level.

Parent Codex owns the final decision.
