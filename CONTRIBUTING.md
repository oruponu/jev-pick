# Contributing to JevPick

Thank you for helping improve JevPick. Keep changes small enough to review and explain the user-visible problem they solve.

## Set up

Install Rustup and your platform's native build tools, then clone the repository. Cargo uses the pinned toolchain in `rust-toolchain.toml`. Fetch dependencies with `cargo fetch --locked`.

Normal development needs no Discord or TypeSafe credentials. Use synthetic fixtures and loopback HTTP mocks. Live testing is optional and must be deliberately performed by an operator; see [the manual checklist](docs/manual-testing.md).

## Before opening a pull request

Run:

```sh
cargo fmt --all -- --check
cargo clippy --all-targets --all-features --locked -- -D warnings
cargo test --all-targets --locked
cargo build --locked
```

Add regression tests for behavior changes, especially around validation, retry deadlines, and interaction state. Tests must not contact paid or live provider APIs.

Use English for documentation, source comments, pull request descriptions, and commit messages. Bot messages are localized into English and Japanese; update both translations when behavior changes.

Prefer cohesive commits such as:

```text
fix: reject unknown probability keys
test: cover cancellation while waiting to retry
docs: clarify Discord installation permissions
```

Explain the change, its motivation, how it was tested, and any checks you could not run. Avoid unrelated reformatting and dependency updates.

## Design boundaries

- Keep validated domain types independent of Discord and HTTP.
- Serialize provider requests with Serde and validate replies before rendering.
- Treat timeouts as uncertain provider outcomes; do not retry them automatically.
- Complete the original deferred response instead of creating additional result messages.
- Preserve user wording and option order.
- Never log user input, credentials, provider response bodies, or complete Discord interactions.
- Never disable TLS verification or let command input select an API endpoint.

Do not add databases, generalized AI abstractions, extra commands, or provider fallbacks without discussing a concrete need first.

## Dependencies and reproducibility

Commit `Cargo.lock`. Dependency updates should include the lockfile and validation on the supported CI platforms. If a toolchain update is necessary, update `rust-toolchain.toml`, `Cargo.toml`, the CI installation step, and README together.

## Reporting bugs

Describe the expected and actual behavior, operating system, Rust version, and reproduction steps. Use made-up input where possible. Redact tokens, private questions, server details, and any raw provider payload.

Do not report exposed credentials in a public issue. Revoke them first and use a private reporting channel offered by the repository host or maintainer.