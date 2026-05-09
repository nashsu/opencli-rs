# Task Completion Checklist

- Run the smallest relevant verification first.
- For Rust changes, prefer `cargo check` before broader `cargo test` or `cargo clippy`.
- For extension changes, run `npm run typecheck` and `npm run build` from `extension/`.
- Confirm no unexpected warnings or regressions were introduced.
- Summarize changed files, what was fixed, and which verification commands were run.

