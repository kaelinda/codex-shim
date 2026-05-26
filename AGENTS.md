# AGENTS.md

This file provides guidance to Codex (Codex.ai/code) when working with code in this repository.

## Project Overview

codex-shim is a local Python/aiohttp server that acts as a model router for Codex Desktop and CLI. It exposes an OpenAI Responses-compatible endpoint on `127.0.0.1:8765` and translates requests to upstream providers (OpenAI chat completions, Anthropic Messages, ChatGPT passthrough). This allows BYOK models to appear as first-class entries in Codex's model picker.

## Development Commands

### Build and Install
```bash
pip install -e ".[dev]"     # install with dev dependencies (pytest, pytest-asyncio)
python -m compileall codex_shim/ -q   # compile check
```

### Testing
```bash
python -m pytest tests/ -q              # run all tests
python -m pytest tests/test_translate.py -q   # single test file
python -m pytest tests/test_translate.py::test_responses_to_chat_text_input -q  # single test
```

### Running the Shim
```bash
codex-shim generate          # regenerate catalog/config
codex-shim start             # start daemon on 127.0.0.1:8765
codex-shim status            # health check + model count
codex-shim list              # show slugs and routes
codex-shim stop              # stop daemon
```

## Architecture

### Core Data Flow
```
Codex Desktop → /v1/responses → ShimServer → upstream provider
                                      ↓
                              translate.py (bidirectional conversion)
                                      ↓
                        Responses-API ↔ Chat Completions / Anthropic Messages
```

### Module Responsibilities

- **`cli.py`**: CLI entrypoint (`codex-shim` command). Manages daemon lifecycle (start/stop/restart), writes Codex config to `~/.codex/config.toml`, handles `patch-app`/`restore-app` for macOS ASAR patching.

- **`server.py`**: `ShimServer` (aiohttp `Application`). Routes `/v1/responses` and `/v1/chat/completions` to the correct upstream based on model slug. Contains `ResponsesStreamState` which translates upstream SSE streams into Codex's Responses-API event sequence (`.added`, `.delta`, `.done`, `.completed` events).

- **`translate.py`**: Stateless request/response translation functions. Converts between three API shapes:
  - `responses_to_chat()` / `responses_to_anthropic()` — inbound (Codex → upstream)
  - `chat_completion_to_response()` / `anthropic_to_response()` — outbound (upstream → Codex)
  - Strips `<think>` blocks from responses via `strip_think()`

- **`settings.py`**: `ModelSettings` loads `~/.codex-shim/models.json`, produces `ShimModel` dataclass instances. Handles slug generation, camelCase/snake_case field aliases, and ChatGPT passthrough availability check.

- **`catalog.py`**: Generates `custom_model_catalog.json` (model picker entries) and `config.toml` (provider config). Each model gets a catalog entry with context window, tool support flags, and reasoning levels.

### Key Design Decisions

- **API key isolation**: Keys live in `~/.codex-shim/models.json` only; the generated catalog never contains them. Server reads keys fresh per request.

- **Streaming translation**: `ResponsesStreamState` in `server.py` is the most complex piece — it accumulates upstream deltas (text, tool calls, reasoning/thinking blocks) and emits properly sequenced Responses-API SSE events. Tool calls and reasoning blocks each get their own `output_index`.

- **Reasoning round-trip**: Anthropic thinking blocks are encoded as `encrypted_content` on `reasoning` items using a `anthropic-thinking-v1:` base64 prefix. This lets Codex preserve them across turns without understanding the format.

- **ChatGPT passthrough**: When `~/.codex/auth.json` has `tokens.access_token`, a synthetic `gpt-5.5` entry routes directly to `chatgpt.com/backend-api/codex/responses` without touching BYOK routes.

### Runtime Files

Generated under `.codex-shim/` (gitignored):
- `custom_model_catalog.json` — model picker catalog
- `config.toml` — opt-in Codex provider config
- `shim.pid`, `shim.log` — daemon state

### Test Patterns

Tests use `pytest-asyncio` with `asyncio_mode = "auto"`. Translation tests verify shape conversion without network calls. Add regression tests when changing translation behavior — tool-call shape bugs are hard to catch by inspection.
