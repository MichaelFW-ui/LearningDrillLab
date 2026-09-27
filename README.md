# Learning Drill Lab

[中文](README.zh-CN.md)

Learning Drill Lab is a Tauri 2 desktop app for learning programming concepts. It generates explanations and exercises, reviews answers, supports follow-up chat, and preserves local topic history.

## Run and build

Install Node.js, Rust, and the platform dependencies for Tauri 2, then run:

```bash
npm ci
npm run tauri dev
```

Build the macOS application:

```bash
npm run tauri build -- --bundles app
```

Run Rust checks with the Tauri manifest:

```bash
cargo fmt --manifest-path src-tauri/Cargo.toml --all
cargo test --manifest-path src-tauri/Cargo.toml --locked
```

## Configuration

In Settings, enter a Chat Completions compatible Base URL and API Key, fetch models, and select one. Bocha, Tavily, and Jina enable optional search and page reading. Topics, exercises, reviews, experiments, and credentials are stored in the local `state.json` shown in Settings. The credentials are currently plaintext. Saves retain a `state.json.bak` backup, which the app can read if the primary file is invalid.

The `curriculum` skill chooses exercise actions and difficulties. Exercises pass context and grading checks. When LibreCodeInterpreter is configured, the `experiment-verification` skill also creates sandbox probes and compares observed output. The exercise panel shows verification status and supports manual sandbox runs with stdout, stderr, and timing.

Set the sandbox base URL and API key in Settings. The client calls `/exec` with LibreChat compatible `code`, `lang`, and optional `session_id` fields, using `x-api-key` authentication. An empty sandbox URL leaves exercises marked as awaiting execution verification.

## Skills

Built-in skills live under `src-tauri/skills/`. Override either skill by placing `skills/curriculum/SKILL.md` or `skills/experiment-verification/SKILL.md` beside the local `state.json`, using matching YAML frontmatter. The app shows the active source in Settings. Rust enforces tool access and quality gates.

## Debugging

AI debug logging is disabled by default. Use `LEARNING_DRILL_LAB_AI_DEBUG=1 npm run tauri dev` during development. Review the log before sharing it: it can contain complete prompts, responses, and fetched pages.
