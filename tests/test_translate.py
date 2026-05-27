from __future__ import annotations

import json

from codex_shim.translate import chat_completion_to_response, responses_to_anthropic, responses_to_chat


def test_responses_to_chat_text_input():
    body = {"model": "slug", "instructions": "System", "input": "Hello", "stream": True, "max_output_tokens": 99}
    out = responses_to_chat(body, "real-model")
    assert out["model"] == "real-model"
    assert out["stream"] is True
    assert out["max_tokens"] == 99
    assert out["messages"] == [{"role": "system", "content": "System"}, {"role": "user", "content": "Hello"}]


def test_responses_function_tools_convert_to_chat_shape():
    body = {
        "model": "slug",
        "input": "Hi",
        "tools": [{"type": "function", "name": "do_work", "description": "Do work", "parameters": {"type": "object"}}],
    }
    out = responses_to_chat(body, "real-model")
    assert out["tools"] == [
        {
            "type": "function",
            "function": {"name": "do_work", "description": "Do work", "parameters": {"type": "object"}},
        }
    ]


def test_responses_to_anthropic_messages():
    body = {"model": "slug", "input": [{"role": "user", "content": [{"type": "input_text", "text": "Hi"}]}]}
    out = responses_to_anthropic(body, "claude-real", 123)
    assert out["model"] == "claude-real"
    assert out["max_tokens"] == 123
    assert out["messages"] == [{"role": "user", "content": "Hi"}]


def test_chat_completion_to_response_strips_think():
    payload = {
        "id": "chatcmpl_1",
        "choices": [{"message": {"role": "assistant", "content": "<think>secret</think>Hello"}}],
    }
    out = chat_completion_to_response(payload, "slug")
    assert out["model"] == "slug"
    assert out["output"][0]["content"][0]["text"] == "Hello"


def test_chat_completion_to_response_preserves_reasoning_content_for_tool_calls():
    payload = {
        "id": "chatcmpl_1",
        "choices": [
            {
                "message": {
                    "role": "assistant",
                    "reasoning_content": "Need the current date before answering.",
                    "content": "Let me check the date.",
                    "tool_calls": [
                        {
                            "id": "call_date",
                            "type": "function",
                            "function": {"name": "get_date", "arguments": "{}"},
                        }
                    ],
                }
            }
        ],
    }

    out = chat_completion_to_response(payload, "deepseek-v4-pro")

    assert [item["type"] for item in out["output"]] == ["reasoning", "message", "function_call"]
    assert out["output"][0]["summary"][0]["text"] == "Need the current date before answering."
    assert out["output"][0]["encrypted_content"].startswith("anthropic-thinking-v1:")


def test_chat_completion_to_response_preserves_minimax_reasoning_details():
    payload = {
        "id": "chatcmpl_1",
        "created": 0,
        "choices": [
            {
                "message": {
                    "role": "assistant",
                    "content": "Answer",
                    "reasoning_details": [
                        {"type": "reasoning.text", "text": "First thought."},
                        {"type": "reasoning.text", "text": "Second thought."},
                    ],
                }
            }
        ],
    }

    out = chat_completion_to_response(payload, "MiniMax-M2")

    assert out["output"][0]["type"] == "reasoning"
    assert out["output"][0]["summary"][0]["text"] == "First thought.\nSecond thought."
    assert out["output"][1]["content"][0]["text"] == "Answer"
    decoded = json.loads(
        __import__("base64").urlsafe_b64decode(
            out["output"][0]["encrypted_content"].removeprefix("anthropic-thinking-v1:").encode("ascii")
        ).decode("utf-8")
    )
    assert decoded["thinking"] == "First thought.\nSecond thought."


def test_responses_to_chat_replays_reasoning_on_same_assistant_tool_call_message():
    body = {
        "model": "deepseek",
        "input": [
            {
                "id": "rs_0",
                "type": "reasoning",
                "summary": [{"type": "summary_text", "text": "Need the current date before answering."}],
            },
            {
                "id": "msg_0",
                "type": "message",
                "role": "assistant",
                "content": [{"type": "output_text", "text": "Let me check the date."}],
            },
            {
                "id": "call_date",
                "type": "function_call",
                "call_id": "call_date",
                "name": "get_date",
                "arguments": "{}",
            },
            {
                "type": "function_call_output",
                "call_id": "call_date",
                "output": "2026-05-26",
            },
        ],
    }

    out = responses_to_chat(body, "deepseek-v4-pro")

    assert out["messages"] == [
        {
            "role": "assistant",
            "content": "Let me check the date.",
            "tool_calls": [
                {
                    "id": "call_date",
                    "type": "function",
                    "function": {"name": "get_date", "arguments": "{}"},
                }
            ],
            "reasoning_content": "Need the current date before answering.",
        },
        {"role": "tool", "tool_call_id": "call_date", "content": "2026-05-26"},
    ]


def test_responses_to_chat_replays_reasoning_when_tool_call_has_no_message_text():
    body = {
        "model": "deepseek",
        "input": [
            {
                "type": "reasoning",
                "summary": [{"type": "summary_text", "text": "Need the current date before answering."}],
            },
            {
                "type": "function_call",
                "call_id": "call_date",
                "name": "get_date",
                "arguments": "{}",
            },
            {
                "type": "function_call_output",
                "call_id": "call_date",
                "output": "2026-05-26",
            },
        ],
    }

    out = responses_to_chat(body, "deepseek-v4-pro")

    assert out["messages"] == [
        {
            "role": "assistant",
            "content": None,
            "tool_calls": [
                {
                    "id": "call_date",
                    "type": "function",
                    "function": {"name": "get_date", "arguments": "{}"},
                }
            ],
            "reasoning_content": "Need the current date before answering.",
        },
        {"role": "tool", "tool_call_id": "call_date", "content": "2026-05-26"},
    ]
