---
name: verify
description: Run clippy lint and all tests to verify code quality. Use after making changes or before committing.
---

Run the following verification steps in order. Stop at the first failure and report the issue.

1. **Format check**: `cargo fmt -- --check`
2. **Lint**: `cargo clippy -- -D warnings`
3. **Test**: `cargo test`

If all steps pass, report success. If any step fails, show the error output and suggest a fix.
