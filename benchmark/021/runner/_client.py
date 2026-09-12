"""Shared Anthropic client factory — uses Bedrock if available, direct API otherwise."""

import os
import anthropic

# Bedrock cross-region inference model IDs
_BEDROCK_HAIKU = (
    "us.anthropic.claude-sonnet-4-6"  # haiku-4-5 unavailable on this account
)
_BEDROCK_SONNET = "us.anthropic.claude-sonnet-4-6"

# Direct API model IDs
_DIRECT_HAIKU = "claude-haiku-4-5-20251001"
_DIRECT_SONNET = "claude-sonnet-4-6"


def _use_bedrock() -> bool:
    return bool(
        os.environ.get("CLAUDE_CODE_USE_BEDROCK")
        or os.environ.get("AWS_BEARER_TOKEN_BEDROCK")
        or os.environ.get("AWS_ACCESS_KEY_ID")
    )


def make_client():
    """Return an Anthropic client configured for Bedrock or direct API."""
    if _use_bedrock():
        return anthropic.AnthropicBedrock(
            aws_region=os.environ.get("AWS_REGION", "us-east-1"),
        )
    return anthropic.Anthropic(api_key=os.environ.get("ANTHROPIC_API_KEY", ""))


# Every variable that can make a scoring call work, in the order make_client() consults them.
# The preflight prints this list when no scorer is reachable: the 2026-08 nightly had
# OPENAI_API_KEY set (the agent's key) and nothing here, and the report said 0% pass rate
# rather than "no credential".
CREDENTIAL_VARS = (
    "AWS_BEARER_TOKEN_BEDROCK",
    "AWS_ACCESS_KEY_ID",
    "ANTHROPIC_API_KEY",
)


def auth_source() -> str:
    """The variable make_client() will authenticate with, or `"none"`."""
    if _use_bedrock():
        for var in ("AWS_BEARER_TOKEN_BEDROCK", "AWS_ACCESS_KEY_ID"):
            if os.environ.get(var):
                return var
        # CLAUDE_CODE_USE_BEDROCK alone means "use Bedrock", leaving the SDK to find an
        # instance role or a profile. Reachable, but not from a variable we can name.
        return "CLAUDE_CODE_USE_BEDROCK"
    return "ANTHROPIC_API_KEY" if os.environ.get("ANTHROPIC_API_KEY") else "none"


def resolved_model(msg) -> str:
    """The model the response says answered, not the id the call asked for.

    Bedrock resolves a cross-region id to a concrete model, and the two are not always the
    same string. Every number the harness stores is attributed from here.
    """
    return getattr(msg, "model", None) or ""


def haiku_model() -> str:
    return _BEDROCK_HAIKU if _use_bedrock() else _DIRECT_HAIKU


def sonnet_model() -> str:
    return _BEDROCK_SONNET if _use_bedrock() else _DIRECT_SONNET
