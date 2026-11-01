pub mod batch;
pub mod benchmark;
pub mod capability_matrix;
pub mod check_sm_credential;
pub mod compile;
pub mod connection_args;
pub mod dispatch;
pub mod doc;
pub mod exec;
pub mod http_guard;
pub mod init;
pub mod install;
pub mod mcp;
pub mod plugin;
pub mod query;
pub mod skill;
pub mod telemetry;
pub mod tool;
pub mod trust;
pub mod tsv;

/// How a subcommand meets the gates.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CommandGate {
    /// Every step that reaches an IRIS server goes through `call_gated` (or `dispatch_gated`), so
    /// the write gate, strict read-only, the environment template, `allow` and the role gate judge
    /// it as they judge a tool call.
    Gated,
    /// Reaches no IRIS server through a path a gate could judge; the string says why.
    Local(&'static str),
}

/// Every executable subcommand, by its full path, and how it meets the gates.
///
/// A command is a leaf of the clap tree: `doc get`, not `doc`. `tests/integration/test_enum_commands.rs`
/// reads the tree off the built binary's `--help` and fails when a command is missing here, when
/// an entry names a command that no longer exists, and (for `Gated`) when the command, run against a
/// listener under a limit that forbids its call, opens a connection. Adding a subcommand means adding
/// its row; there is no default.
pub const COMMAND_GATES: &[(&str, CommandGate)] = &[
    ("benchmark", CommandGate::Gated),
    ("batch", CommandGate::Gated),
    ("capability-matrix", CommandGate::Gated),
    (
        "check-sm-credential",
        CommandGate::Local("reads the OS keychain and VS Code's credential store, then exits"),
    ),
    ("compile", CommandGate::Gated),
    ("doc get", CommandGate::Gated),
    ("doc put", CommandGate::Gated),
    ("exec", CommandGate::Gated),
    (
        "init",
        CommandGate::Local(
            "writes a workspace file; lists Docker containers, never an IRIS server",
        ),
    ),
    (
        "install",
        CommandGate::Local("resolves the package manifest on disk and writes the lock file"),
    ),
    (
        "mcp",
        CommandGate::Local(
            "starts the server; discovery identifies a listener without credentials and every \
             tool call meets the pipeline (swept by core's test_enum_no_leaks)",
        ),
    ),
    ("query", CommandGate::Gated),
    (
        "skill install",
        CommandGate::Local("fetches the skill pack from GitHub and writes agent directories"),
    ),
    (
        "skill list",
        CommandGate::Local("lists the skill pack and the files on disk"),
    ),
    (
        "skill status",
        CommandGate::Local("lists the skill files on disk"),
    ),
    (
        "telemetry export",
        CommandGate::Local("reads the local JSONL telemetry sink"),
    ),
    ("tool", CommandGate::Gated),
    (
        "trust",
        CommandGate::Local("writes a trust entry to the operator file; needs a terminal"),
    ),
    (
        "<external>",
        CommandGate::Local(
            "hands an unknown subcommand to an iris-agentic-dev-<name> binary on PATH, which the \
             operator installed and which has no connection of ours",
        ),
    ),
];
