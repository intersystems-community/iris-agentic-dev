# Invariants the tests enforce

Each round of gate fixes closed a list of gaps found one at a time. The same
classes came back in places nobody had listed: a tool marked `Local` that ran code on the default
server, a CLI command outside the gated pipeline, call sites using the startup HTTP client, a
setting left off the trust list, a store shared between HTTP sessions. These tests enumerate
everything of each kind from the real registry, clap tree, source tree or struct, and fail when
anything is undeclared. Adding one of the things below means adding its declaration in the same
change. There is no default.

| You add                     | You declare it in                                                                                 | The test that fails without it                                               |
| --------------------------- | ------------------------------------------------------------------------------------------------- | ---------------------------------------------------------------------------- |
| a tool                      | `write_gate::CLASSIFICATION` (tier, `Iris` or `Local` with a reason) and `tool_to_category`       | `test_gate_classification`, `test_enum_gate_classes`, `test_enum_no_leaks`   |
| a `Local` tool              | the row's `.local(reason)` and `LOCAL_TOOLS` in `test_enum_gate_classes.rs`                       | `test_enum_gate_classes`, then `test_enum_no_leaks` if it opens a connection |
| a CLI subcommand            | `cmd::COMMAND_GATES` (`Gated`, or `Local` with a reason) and, if gated, an invocation in the test | `bin_integration::test_enum_commands`                                        |
| a configuration key         | `config_sources::SETTINGS` (loosening, tightening, neutral or operator-only, with an example)     | `test_enum_config_keys`                                                      |
| an environment variable     | `testing::BEHAVIOR_ENV_VARS` and `config_sources::LAUNCH_SETTINGS`                                | `test_enum_config_keys`                                                      |
| an HTTP client              | `iris/http.rs`; a file that builds its own is listed in `OWN_CLIENT` with the host it talks to    | `test_enum_http_clients`                                                     |
| a use of the startup client | `STARTUP_CLIENT_USES`, or take `http_client()` / `conn_client(&conn)` instead                     | `test_enum_http_clients`                                                     |
| a field on `IrisTools`      | `tools::SESSION_FIELDS` (`PerSession`, reset in `for_new_session`, or `Shared` with the reason)   | `test_enum_session_stores`                                                   |

## What each one checks

- **Gate classes.** `CLASSIFICATION` names every registered tool. A `Local` row says why it touches
  no IRIS server, an `Iris` row maps to a tool category, and the list of `Local` tools is closed:
  a reviewer sees any name added to `LOCAL_TOOLS`.
- **No leaks.** Every tool, in every toolset, is called through the MCP pipeline with minimal
  arguments from its schema, under strict read-only, a `live` template that allows only `search`,
  and closed writes. The default connection is a loopback listener that counts connections and
  answers 404. A `Local` tool never opens one; an `Iris` tool opens one only when the table says the
  setting allows the call. The skip list may only shrink.
- **Commands.** The command tree is read off the built binary's `--help` and compared with
  `COMMAND_GATES`. Each gated command runs against the same listener under the same three limits.
  The version probe and the system-mode read come before any gate and are not counted; every
  request after them is.
- **Config keys.** The keys come from the serde structs and the raw-table reads in the source. Each
  row's example document is run through `loosening_settings` and `operator_only_items`, so a key
  declared loosening that the trust machinery does not see fails. The product writes the
  environment in four listed places, all for variables that cannot loosen.
- **HTTP clients.** A source scan: no client is built outside `iris/http.rs` except the listed
  non-IRIS hosts, `self.client` is read only where listed, and a handler that names its servers
  builds each one's client from its connection.
- **Session stores.** The struct definition and `for_new_session_sweeping` are read off the source
  and compared with `SESSION_FIELDS`; the live `Arc`s of the public fields are compared too.

Every allowlist in these tests has a companion test that fails when an entry no longer matches
anything, so each list can only shrink.
