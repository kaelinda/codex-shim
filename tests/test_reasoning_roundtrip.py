"""Test reasoning roundtrip in multi-turn conversations."""

import json

from codex_shim.translate import responses_to_chat


def _decode_thinking_text(encrypted_content: str) -> str:
    """Extract thinking text from encrypted_content for comparison."""
    import base64

    from codex_shim.translate import _THINKING_MAGIC

    if not encrypted_content.startswith(_THINKING_MAGIC):
        return ""
    blob = encrypted_content[len(_THINKING_MAGIC) :]
    raw = base64.urlsafe_b64decode(blob.encode("ascii"))
    data = json.loads(raw.decode("utf-8"))
    return data.get("thinking", "")


def test_reasoning_roundtrip_chat():
    """Simulate a second turn where Codex includes reasoning + message from turn 1."""
    input_items = [
        {"type": "message", "role": "user", "content": [{"type": "input_text", "text": "Hello"}]},
        {
            "type": "reasoning",
            "summary": [{"type": "summary_text", "text": "The user is greeting me."}],
            "encrypted_content": "anthropic-thinking-v1:eyJ0eXBlIjoidGhpbmtpbmciLCJ0aGlua2luZyI6IlRoZSB1c2VyIGlzIGdyZWV0aW5nIG1lLiIsInNpZ25hdHVyZSI6IiJ9",
        },
        {
            "type": "message",
            "role": "assistant",
            "content": [{"type": "output_text", "text": "Hi there!"}],
        },
        {"type": "message", "role": "user", "content": [{"type": "input_text", "text": "How are you?"}]},
    ]

    result = responses_to_chat(
        {"input": input_items, "thinking": True, "stream": True, "model": "deepseek"},
        upstream_model="deepseek-chat",
    )

    assert result["thinking"] == {"type": "enabled"}
    messages = result["messages"]

    # Should have: user, assistant (with reasoning), user
    assert len(messages) == 3, f"Expected 3 messages, got {len(messages)}: {json.dumps(messages, indent=2)}"

    assistant = messages[1]
    assert assistant["role"] == "assistant"
    assert "reasoning_content" in assistant, (
        f"reasoning_content missing from assistant message: {json.dumps(assistant, indent=2)}"
    )
    assert "The user is greeting me" in assistant["reasoning_content"]

    user = messages[2]
    assert user["role"] == "user"


def test_reasoning_at_end_of_conversation():
    """Reasoning as the last item (no following assistant message)."""
    input_items = [
        {"type": "reasoning", "summary": [{"type": "summary_text", "text": "My final thought."}], "encrypted_content": None},
        {"type": "message", "role": "user", "content": [{"type": "input_text", "text": "New question"}]},
    ]

    result = responses_to_chat(
        {"input": input_items, "stream": True, "model": "deepseek"},
        upstream_model="deepseek-chat",
    )

    messages = result["messages"]
    # Since reasoning doesn't have a following assistant message, it creates a standalone
    assistant_msgs = [m for m in messages if m["role"] == "assistant"]
    assert len(assistant_msgs) == 1
    assert "reasoning_content" in assistant_msgs[0]
    assert assistant_msgs[0]["reasoning_content"] == "My final thought."
    # thinking should be auto-enabled
    assert result["thinking"] == {"type": "enabled"}


def test_reasoning_from_encrypted_content():
    """Reasoning item with only encrypted_content (no summary)."""
    input_items = [
        {
            "type": "reasoning",
            "summary": [],
            "encrypted_content": "anthropic-thinking-v1:eyJ0eXBlIjoidGhpbmtpbmciLCJ0aGlua2luZyI6IkRlY29kZWQgdGhpbmtpbmciLCJzaWduYXR1cmUiOiIifQ==",
        },
        {
            "type": "message",
            "role": "assistant",
            "content": [{"type": "output_text", "text": "Answer"}],
        },
        {"type": "message", "role": "user", "content": [{"type": "input_text", "text": "Next"}]},
    ]

    result = responses_to_chat(
        {"input": input_items, "thinking": True, "stream": True},
        upstream_model="deepseek-chat",
    )

    assistant = result["messages"][0]
    assert "Decoded thinking" in assistant.get("reasoning_content", "")


def test_no_reasoning_preserves_thinking():
    """First turn with thinking=true, no reasoning in input."""
    result = responses_to_chat(
        {"input": [{"type": "message", "role": "user", "content": [{"type": "input_text", "text": "Hi"}]}],
         "thinking": True, "stream": True},
        upstream_model="deepseek-chat",
    )

    assert result["thinking"] == {"type": "enabled"}
    # No reasoning_content on assistant message (it's the first turn)
    for m in result["messages"]:
        assert "reasoning_content" not in m, f"Unexpected reasoning_content: {m}"


def test_thinking_disabled_but_reasoning_present():
    """If Codex sends thinking=false but reasoning exists in input, auto-enable it."""
    input_items = [
        {"type": "reasoning", "summary": [{"type": "summary_text", "text": "Think."}], "encrypted_content": None},
        {
            "type": "message",
            "role": "assistant",
            "content": [{"type": "output_text", "text": "Hi"}],
        },
        {"type": "message", "role": "user", "content": [{"type": "input_text", "text": "Q"}]},
    ]

    result = responses_to_chat(
        {"input": input_items, "thinking": False, "stream": True},
        upstream_model="deepseek-chat",
    )

    # Auto-enable thinking because reasoning_content is present
    assert result["thinking"] == {"type": "enabled"}
    assert "reasoning_content" in result["messages"][0]


def test_moonshot_legacy_model_drops_thinking_option():
    result = responses_to_chat(
        {
            "input": [{"type": "message", "role": "user", "content": [{"type": "input_text", "text": "Hi"}]}],
            "thinking": True,
            "stream": True,
        },
        upstream_model="moonshot-v1-32k",
        provider="moonshot",
    )

    assert "thinking" not in result


def test_kimi_model_keeps_thinking_option():
    result = responses_to_chat(
        {
            "input": [{"type": "message", "role": "user", "content": [{"type": "input_text", "text": "Hi"}]}],
            "thinking": True,
            "stream": True,
        },
        upstream_model="kimi-k2.6",
        provider="moonshot",
    )

    assert result["thinking"] == {"type": "enabled", "keep": "all"}


def test_generic_chat_provider_can_opt_into_thinking_passthrough():
    result = responses_to_chat(
        {
            "input": [{"type": "message", "role": "user", "content": [{"type": "input_text", "text": "Hi"}]}],
            "thinking": {"type": "enabled"},
            "stream": True,
        },
        upstream_model="reasoning-model",
        provider="generic-chat-completion-api",
    )

    assert result["thinking"] == {"type": "enabled"}


def test_encrypted_content_preserved_in_reasoning_only():
    """The _reasoning_only message should carry encrypted_content for the Anthropic path."""
    from codex_shim.translate import _responses_input_to_messages

    encrypted = "anthropic-thinking-v1:eyJ0eXBlIjoidGhpbmtpbmciLCJ0aGlua2luZyI6InRlc3QiLCJzaWduYXR1cmUiOiJzaWcifQ=="
    input_items = [
        {
            "type": "reasoning",
            "summary": [{"type": "summary_text", "text": "test"}],
            "encrypted_content": encrypted,
        },
        {"type": "message", "role": "user", "content": [{"type": "input_text", "text": "Q"}]},
    ]

    messages = _responses_input_to_messages(input_items)
    reasoning_msgs = [m for m in messages if m.get("_reasoning_only")]
    assert len(reasoning_msgs) == 1
    assert reasoning_msgs[0].get("encrypted_content") == encrypted, (
        f"encrypted_content not preserved: {reasoning_msgs[0]}"
    )
