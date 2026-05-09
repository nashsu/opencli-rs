# Style and Conventions

- Rust code uses the 2021 edition and strict typing.
- Prefer small, focused modules and shared workspace dependencies from the root `Cargo.toml`.
- Use idiomatic Rust naming: `snake_case` for functions/modules, `CamelCase` for types.
- The codebase uses explanatory comments for non-obvious behavior, especially around browser automation and protocol handling.
- TypeScript in the extension is `strict: true`, uses ES modules, and targets ES2022.
- Existing code favors explicit async handling, `Result`-based error paths, and clear separation between protocol, transport, and execution layers.

