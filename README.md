# Learning Drill Lab

[中文](README.zh-CN.md)

Learning Drill Lab is a desktop app for practicing programming concepts with an AI tutor. Give it a topic, and it builds a study session with an explanation, exercises, answer review, and follow-up chat.

The current prompts are written for Chinese tutoring. The app itself is a Rust/Dioxus desktop app.

## Features

- Topic-based explanations with code examples and common mistakes.
- Exercise generation with a validation pass before questions are shown.
- Answer review that can accept a correct challenge to a flawed question.
- Follow-up chat tied to the current topic, exercises, and attempts.
- Local topic history.
- Optional web grounding through Bocha or Tavily.
- Optional page reading through Jina Reader.

## Quick Start

```bash
cargo run
```

Open **Settings** in the app and configure:

- `Base URL`: a Chat Completions compatible endpoint, for example `https://api.openai.com/v1`
- `API Key`
- a model from the fetched model list

The app can run without search keys. Search and page reading are optional.

## Optional Search Setup

Learning Drill Lab exposes two tools to the model when the relevant settings are present.

### `web_search`

Enabled by either:

- Bocha API key
- Tavily API key

Tavily is tried first when configured. If Tavily fails, Bocha is used as fallback. Each search tool call sends one query and returns up to 10 results.

For a Tavily-compatible proxy, set `Tavily HTTP Base URL` to the proxy's Tavily-style HTTP API base. Do not put an MCP endpoint in this field. A value ending in `/search` is also accepted.

### `web_fetch`

Enabled by default as a public Jina Reader fetch tool.

- The model chooses which URLs to read.
- At most 5 URLs are fetched per call.
- Fetches are serial, with a 3 second delay between URLs.
- Public Jina Reader is used first without an API key.
- A configured Jina API key is used only after public Reader returns an auth or rate-limit response.
- Jina is not used for search.

## Local Data

The app stores state on your machine using the OS config directory. The settings page shows the exact `state.json` path.

Stored in `state.json`:

- API keys
- selected model and endpoints
- topic history
- exercises
- answers and reviews

The file is plain JSON. Treat it as sensitive.

## Debug Logging

AI debug logging is disabled by default.

To enable it:

```bash
LEARNING_DRILL_LAB_AI_DEBUG=1 cargo run
```

When enabled, the app writes full AI requests, responses, tool results, fetched page text, and JSON repair attempts to a local `ai-debug.log` file. Do not share that log unless you have reviewed it.

## Development

```bash
cargo fmt
cargo check
cargo test
```

## Notes

- This is an early desktop app, not a packaged release.
- API keys are currently stored in plain text.
- The UI and prompts are still tuned around the author's workflow.
- The app expects a Chat Completions style API with tool call support for search/fetch.

