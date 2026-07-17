# Quickstart: Skill Install (061)

## For users

```bash
# Install the full pack for Claude Code and OpenCode (default)
iris-agentic-dev skill install

# Install for a specific agent
iris-agentic-dev skill install --agent copilot   # writes to .github/instructions/

# Install a specific skill only
iris-agentic-dev skill install pyprod

# See what would be installed
iris-agentic-dev skill install --dry-run

# Check install status
iris-agentic-dev skill list
```

## For developers testing the implementation

```bash
# Unit tests (no IRIS, no network)
cargo test -p iris-agentic-dev-core skill_install

# Verify path resolution
cargo test -p iris-agentic-dev-core skill_install::tests::test_claude_code_path
cargo test -p iris-agentic-dev-core skill_install::tests::test_opencode_path
cargo test -p iris-agentic-dev-core skill_install::tests::test_managed_by_detection
cargo test -p iris-agentic-dev-core skill_install::tests::test_collision_skip

# Integration test (requires network — fetches from GitHub)
cargo test -p iris-agentic-dev-core skill_install::tests::test_fetch_pack -- --ignored
```
