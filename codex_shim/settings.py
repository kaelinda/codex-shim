from __future__ import annotations

from dataclasses import dataclass, field
import json
from pathlib import Path
import re
from typing import Any


DEFAULT_SETTINGS = Path.home() / ".codex-shim" / "models.json"
DEFAULT_CODEX_AUTH = Path.home() / ".codex" / "auth.json"
DEFAULT_HOST = "127.0.0.1"
DEFAULT_PORT = 8765
PROVIDER_NAME = "codex_shim"
OPENAI_CHAT_PROVIDERS = {
    "openai",
    "generic-chat-completion-api",
    "deepseek",
    "minimax",
    "moonshot",
    "dashscope",
    "volcengine",
}
SUPPORTED_PROVIDERS = OPENAI_CHAT_PROVIDERS | {"anthropic"}


def chatgpt_passthrough_available(auth_path: Path | None = None) -> bool:
    """Return True if ~/.codex/auth.json holds a usable Codex access token."""
    if auth_path is None:
        import sys as _sys

        auth_path = getattr(_sys.modules[__name__], "DEFAULT_CODEX_AUTH")
    expanded = Path(auth_path).expanduser()
    if not expanded.exists():
        return False
    try:
        data = json.loads(expanded.read_text())
    except (OSError, json.JSONDecodeError):
        return False
    tokens = data.get("tokens") if isinstance(data, dict) else None
    if not isinstance(tokens, dict):
        return False
    return bool(tokens.get("access_token"))


def slugify(value: str) -> str:
    slug = re.sub(r"[^a-zA-Z0-9]+", "-", value.strip().lower()).strip("-")
    return slug or "model"


@dataclass(frozen=True)
class ShimModel:
    slug: str
    model: str
    display_name: str
    provider: str
    base_url: str
    api_key: str = ""
    index: int = 0
    max_context_limit: int | None = None
    max_output_tokens: int | None = None
    no_image_support: bool = False
    extra_headers: dict[str, str] = field(default_factory=dict)
    raw: dict[str, Any] = field(default_factory=dict)

    @property
    def is_anthropic(self) -> bool:
        return self.provider == "anthropic"

    @property
    def is_openai_chat(self) -> bool:
        return self.provider in OPENAI_CHAT_PROVIDERS


class ModelSettings:
    def __init__(self, path: Path | None = None):
        self.path = Path(path or DEFAULT_SETTINGS).expanduser()

    def load(self) -> list[ShimModel]:
        if not self.path.exists():
            if self.path == DEFAULT_SETTINGS:
                return []
            raise FileNotFoundError(self.path)
        data = json.loads(self.path.read_text())
        rows = _model_rows(data)
        model_counts: dict[str, int] = {}
        for row in rows:
            model = str(row.get("model") or "").strip()
            if model:
                model_counts[model] = model_counts.get(model, 0) + 1

        used: set[str] = set()
        models: list[ShimModel] = []
        for fallback_index, row in enumerate(rows):
            model = str(row.get("model") or "").strip()
            provider = str(row.get("provider") or "").strip()
            base_url = str(_field(row, "base_url", "baseUrl") or "").strip().rstrip("/")
            if not model or not provider or not base_url:
                continue

            index = int(row.get("index", fallback_index))
            display_name = str(_field(row, "display_name", "displayName", default=model)).strip()
            slug_base = str(row.get("slug") or (display_name if model_counts.get(model, 0) > 1 else model))
            slug = slugify(slug_base)
            if slug in used:
                slug = f"{slug}-{index}"
            while slug in used:
                slug = f"{slug}-{len(used)}"
            used.add(slug)

            extra_headers = {
                str(k): str(v)
                for k, v in (_field(row, "extra_headers", "extraHeaders", default={}) or {}).items()
                if v is not None
            }
            models.append(
                ShimModel(
                    slug=slug,
                    model=model,
                    display_name=display_name,
                    provider=provider,
                    base_url=base_url,
                    api_key=str(_field(row, "api_key", "apiKey", default="")),
                    index=index,
                    max_context_limit=_int_or_none(_field(row, "max_context_limit", "maxContextLimit")),
                    max_output_tokens=_int_or_none(_field(row, "max_output_tokens", "maxOutputTokens")),
                    no_image_support=bool(_field(row, "no_image_support", "noImageSupport", default=False)),
                    extra_headers=extra_headers,
                    raw=row,
                )
            )
        return models

    def by_slug_or_model(self, requested: str) -> ShimModel | None:
        models = self.load()
        by_slug = {m.slug: m for m in models}
        if requested in by_slug:
            return by_slug[requested]
        matches = [m for m in models if m.model == requested]
        if len(matches) == 1:
            return matches[0]
        return None


def _model_rows(data: Any) -> list[dict[str, Any]]:
    if not isinstance(data, dict):
        return []
    rows = data.get("models")
    if rows is None:
        rows = data.get("customModels", [])
    if not isinstance(rows, list):
        return []
    return [row for row in rows if isinstance(row, dict)]


def _field(row: dict[str, Any], *keys: str, default: Any = None) -> Any:
    for key in keys:
        if key in row:
            return row[key]
    return default


def _int_or_none(value: Any) -> int | None:
    if value in (None, ""):
        return None
    try:
        return int(value)
    except (TypeError, ValueError):
        return None


def default_model_slug(models: list[ShimModel]) -> str:
    if chatgpt_passthrough_available():
        return "gpt-5.5"
    if models:
        return models[0].slug
    return "gpt-5.5"
