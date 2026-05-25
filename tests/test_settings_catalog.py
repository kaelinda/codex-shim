from __future__ import annotations

import json

import pytest

from codex_shim import cli
from codex_shim.catalog import catalog_entry
from codex_shim.settings import FactorySettings


def test_duplicate_models_get_unique_display_slugs(tmp_path):
    settings = tmp_path / "settings.json"
    settings.write_text(
        json.dumps(
            {
                "customModels": [
                    {"model": "gpt-5.5", "displayName": "Fast High", "provider": "openai", "baseUrl": "http://x/v1", "index": 1},
                    {"model": "gpt-5.5", "displayName": "Fast Low", "provider": "openai", "baseUrl": "http://x/v1", "index": 2},
                ]
            }
        )
    )
    models = FactorySettings(settings).load()
    assert [m.slug for m in models] == ["fast-high", "fast-low"]


def test_catalog_preserves_context_and_visibility():
    model = FactorySettingsFixture.one()
    entry = catalog_entry(model)
    assert entry["slug"] == "claude-opus"
    assert entry["visibility"] == "list"
    assert entry["context_window"] == 200000
    assert "free" in entry["available_in_plans"]


def test_default_missing_factory_settings_allows_chatgpt_only(monkeypatch, tmp_path):
    missing = tmp_path / "missing-default.json"
    monkeypatch.setattr("codex_shim.settings.DEFAULT_FACTORY_SETTINGS", missing)
    assert FactorySettings(missing).load() == []


def test_cli_load_models_missing_custom_settings_has_actionable_error(tmp_path):
    missing = tmp_path / "missing.json"
    with pytest.raises(SystemExit) as exc:
        cli._load_models(missing)
    assert "Settings file not found" in str(exc.value)
    assert "--settings /path/to/settings.json" in str(exc.value)


def test_cli_resolves_chatgpt_passthrough_slug_without_factory_models():
    assert cli._resolve_model_slug([], "gpt-5.5") == "gpt-5.5"
    assert cli._resolve_model_slug([], "openai-gpt-5-5") == "gpt-5.5"


def test_list_models_includes_chatgpt_passthrough(monkeypatch, capsys):
    monkeypatch.setattr(cli, "_load_models", lambda _settings_path: [])
    assert cli.list_models("unused") == 0
    assert "gpt-5.5" in capsys.readouterr().out


def test_cli_load_models_invalid_json_has_actionable_error(tmp_path):
    settings = tmp_path / "settings.json"
    settings.write_text("{")
    with pytest.raises(SystemExit) as exc:
        cli._load_models(settings)
    assert "Settings file is not valid JSON" in str(exc.value)


class FactorySettingsFixture:
    @staticmethod
    def one():
        import tempfile
        from pathlib import Path

        path = Path(tempfile.mkdtemp()) / "settings.json"
        path.write_text(
            json.dumps(
                {
                    "customModels": [
                        {
                            "model": "claude-opus",
                            "displayName": "Claude Opus",
                            "provider": "anthropic",
                            "baseUrl": "http://anthropic",
                            "maxContextLimit": 200000,
                        }
                    ]
                }
            )
        )
        return FactorySettings(path).load()[0]

