# JevPick

A small, self-hosted Discord bot that asks [Jev](https://docs.typesafe.ai/primitives/choice) to choose between two to four options.

Use `/jev` with a question and your own choices. JevPick returns the selected option and each option's model probability in one public embed. It preserves your wording and order, and replies in Japanese or English based on the question.

**Status:** Initial implementation. Automated tests use local HTTP mocks; live Discord and TypeSafe verification is a separate operator step. JevPick is an independent project, not an official TypeSafe or Discord product.

## What it does

- Offers one slash command in one configured Discord server.
- Uses Jev's structured Choice API with stable option IDs, `A` through `D`.
- Validates inputs, response IDs, probability distributions, and token usage.
- Limits concurrent evaluations and retries only rate-limit or overload responses.
- Keeps no conversation history or database.

JevPick does not browse the web, explain its reasoning, conduct a poll, or provide a fair random draw. Probabilities are model assessments, **not accuracy scores or guarantees**. Supply relevant facts in `context`.

## Quick start

### Requirements

- [Rustup](https://rustup.rs/) and a native build toolchain. This repository pins **Rust 1.98.1**, Edition 2024, in `rust-toolchain.toml`.
- A Discord application with a bot token and a server where you can install it.
- A TypeSafe API key with access to Jev.
- Outbound HTTPS and Discord Gateway connectivity.

On Windows, install the Visual Studio C++ Build Tools and Windows SDK. Rustls's cryptographic dependency also requires CMake and a C compiler for source builds.

Clone or download this repository, then open a terminal in its root.

### 1. Install your Discord bot

In the [Discord Developer Portal](https://discord.com/developers/applications):

1. Create an application and obtain its bot token.
2. Configure a **Guild Install** with the `bot` and `applications.commands` scopes.
3. Grant **View Channels**, **Send Messages**, and **Embed Links** in the intended channel.
4. Install the application in your server.
5. Enable Developer Mode in Discord and copy the server ID.

Administrator permission and privileged intents are unnecessary. Leave Message Content, Server Members, and Presence intents disabled. Use Discord's integration and channel permissions to restrict who can run the command.

### 2. Configure it

Copy `.env.example` to `.env`:

```sh
# Linux / macOS
cp .env.example .env
```

```powershell
# Windows PowerShell
Copy-Item .env.example .env
```

Fill in the three required values:

```dotenv
DISCORD_TOKEN=your_discord_bot_token
TYPESAFE_API_KEY=your_typesafe_api_key
DISCORD_GUILD_ID=your_server_id
```

Never commit this file or paste credentials into an issue.

| Variable | Required | Default | Purpose |
| --- | --- | --- | --- |
| `DISCORD_TOKEN` | Yes | — | Discord bot token |
| `TYPESAFE_API_KEY` | Yes | — | TypeSafe API key |
| `DISCORD_GUILD_ID` | Yes | — | Nonzero numeric server ID |
| `JEV_MODEL` | No | `jev-latest` | Model alias or versioned model ID |
| `RUST_LOG` | No | `warn,jev_pick=info` | Log filter |

Process environment variables override `.env`. The loader looks in the current directory and then its parents. A missing `.env` is fine; malformed files, blank required values, invalid server IDs, and explicitly empty model names stop startup. Error messages do not print configuration values.

The `jev-latest` alias can change over time. Use a supported versioned ID to pin the provider model. Replies show the actual model returned by the API.

### 3. Run it

```sh
cargo run --release --locked
```

Startup creates or updates only `/jev` in the configured server, preserving other commands belonging to the application. The bot uses a Gateway connection; no public web server is required. Keep one process running and use Ctrl+C to stop it.

For deployments, build with `cargo build --release --locked`, run the executable in `target/release/` (with `.exe` on Windows), and supply the environment variables through your process manager.

## Usage

```text
/jev question:What should I have for dinner? a:Curry b:Udon c:Sushi context:I want something warm.
/jev question:今日の晩ごはんは？ a:カレー b:うどん c:寿司 context:温かい麺類が食べたい
```

| Argument | Required | Limit |
| --- | --- | --- |
| `question` | Yes | 300 Unicode scalar values |
| `a`, `b` | Yes | 120 each |
| `c`, `d` | No | 120 each |
| `context` | No | 500 |

Limits apply after line endings are normalized and surrounding whitespace is trimmed. `d` requires `c`. Supplied choices cannot be blank or duplicate another normalized choice exactly. Case, width, and Unicode normalization are not folded together. Blank context is treated as absent. Inputs are rejected rather than shortened; Discord also applies command-option length limits before submission.

The bot uses a local two-language heuristic: a question containing Japanese kana or CJK ideographs selects Japanese; all other questions select English. Mixed questions containing those scripts select Japanese. Romanized Japanese, Chinese-only text, and very short questions can be ambiguous. The Discord interface language and the language of the choices do not override the question. The bot does not translate your input or call an extra language-detection API.

Results include the selected option, original question, optional context, choices in their original order, percentages rounded to one decimal place, and the actual model name. Rounded percentages may total 99.9% or 100.1%. The separate API `confidence` metric is validated internally and is not displayed.

## Privacy and operating limits

**Questions, choices, and context are sent to TypeSafe. Successful replies publish all three in the Discord channel.** Do not enter information you would not share with either audience.

JevPick does not add usernames, Discord IDs, channel history, or profile data to the Jev request. It stores no database and does not log input text, credentials, or HTTP bodies. Logs contain operational metadata such as an interaction ID, candidate count, duration, attempt number, HTTP status, model ID, and token usage. Only JevPick's own log targets are enabled, even with `RUST_LOG=trace`, to prevent dependency diagnostics from exposing payloads. This does not mean Discord or TypeSafe retain no data; their own service policies still apply.

All replies explicitly suppress mentions. User text is escaped for Discord Markdown only when displayed; the API receives the normalized original text.

| Control | Behavior |
| --- | --- |
| User cooldown | 10 seconds, in memory |
| Concurrent evaluations | 2 per process; excess requests are rejected immediately |
| Connection timeout | 3 seconds |
| Each HTTP attempt | 10 seconds, including the response body |
| Whole evaluation | 30 seconds, including retry waits |
| Retry limit | 3 attempts total, only for HTTP 429 or 529 |
| Retry wait | 500 ms, then 1 second; a longer valid `Retry-After` takes precedence |
| Success body limit | 64 KiB, enforced during reading |

A retry that cannot fit its wait before the deadline is skipped. Timeouts, connection errors, invalid responses, and other HTTP errors are not retried. Redirects are disabled and TLS certificates are verified. The production API endpoint is fixed in the program.

Input errors, cooldowns, and capacity rejections are visible only to the invoking user. Accepted requests are deferred publicly; a later API error edits that public response. Discord delivery failures never trigger another Jev evaluation.

Provider calls may incur charges. Local cancellation, timeouts, or shutdown do not guarantee provider-side cancellation or a refund. Cooldowns and concurrency limits are not spending caps, and running multiple processes multiplies those limits.

## Development

```sh
cargo fmt --all -- --check
cargo clippy --all-targets --all-features --locked -- -D warnings
cargo test --all-targets --locked
cargo build --locked
```

Tests require no tokens. After dependencies are fetched, they run without contacting Discord or TypeSafe; HTTP tests bind only to loopback. CI runs the same checks on Linux and Windows. Fixtures are synthetic examples, not measured model output.

The main boundaries are `decision` (validated inputs), `config` (startup settings), `language` (response language), `jev` (provider transport and validation), `render` (pure display data), and `discord` (interaction lifecycle). Library exports support testing and are not a stable public Rust API.

See [CONTRIBUTING.md](CONTRIBUTING.md) for development conventions and [manual verification](docs/manual-testing.md) for checks that require a real Discord server.

## Troubleshooting

| Symptom | Check |
| --- | --- |
| Startup reports missing configuration | Run from the directory containing `.env`, or export the required variables. |
| `/jev` is missing | Check the server ID, guild installation, application-command scope, and startup registration result. Commands are not registered globally. |
| The bot cannot reply | Check channel overrides for View Channels, Send Messages, and Embed Links. |
| Authentication or request configuration error | Check the TypeSafe key and model ID without sharing the secret. |
| Rate-limited or overloaded response | Wait before retrying; inspect provider limits and safe status logs. |
| Unexpected response | The provider reply failed validation. Update the integration against the [API reference](https://docs.typesafe.ai/api); do not repair probabilities silently. |
| Model choice seems unsuitable | Include missing facts in `context`. A valid response is not a guarantee of sound judgment. |

## Contributing and license

Focused bug reports and pull requests are welcome. Include a reproducible case and the relevant test output, with all credentials and private inputs removed.

Licensed under the [MIT License](LICENSE).
