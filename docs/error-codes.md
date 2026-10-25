# Error codes

Codes iad answers with that a caller may need to act on. Each refusal also carries a `refusal`
object whose `kind` says who can lift it: `acknowledgement_required` (repeat the call with
`confirm` or `acknowledgePhi`), `operator_limit` (a setting in the operator file or launch
settings), `untrusted_setting` (trust the file with `iris-agentic-dev trust`) or `fixed_rule`
(not lifted). A code marked "not a refusal" is an ordinary error.

The HTTP transport's refusals (a missing or wrong token, a bad `Host` or `Origin`) are HTTP
401, 403 or 421 responses before MCP, not tool errors.

## Error code registry

| Code                                      | Kind                     | Raised when                                                                                                                               |
| ----------------------------------------- | ------------------------ | ----------------------------------------------------------------------------------------------------------------------------------------- |
| `SUBJECT_CONFIRM_REQUIRED`                | acknowledgement required | a write on a subject server without `confirm: true`                                                                                       |
| `SUBJECT_SCM_WRITE_BLOCKED`               | fixed rule               | a source-control write on a subject server                                                                                                |
| `CONFIG_INVALID`                          | not a refusal            | a configuration file or entry fails to parse or validate                                                                                  |
| `TARGET_NOT_RESOLVED`                     | not a refusal (internal) | a handler reads the target outside the pipeline                                                                                           |
| `SERVICE_ACCOUNT_WITHHELD`                | operator limit           | the service account was withheld from this server                                                                                         |
| `SERVICE_ACCOUNT_UNCONFIGURED_FOR_SERVER` | operator limit           | a service account is set but this pool member has none                                                                                    |
| `SERVICE_ACCOUNT_HTTP_DENIED`             | operator limit           | the service account cannot use the HTTP path                                                                                              |
| `SERVICE_ACCOUNT_LOGIN_FAILED`            | operator limit           | the docker/SSH session could not drop to the service account                                                                              |
| `SERVICE_ACCOUNT_IDENTITY_MISMATCH`       | operator limit           | IRIS reports a different `$USERNAME` than expected                                                                                        |
| `SERVICE_ACCOUNT_INVALID`                 | not a refusal            | the configured username has control characters                                                                                            |
| `SESSION_IDENTITY_CHANGED`                | operator limit           | a WebSocket session's identity no longer matches the configured one                                                                       |
| `DOCKER_TERMINAL_LOGIN_REQUIRED`          | operator limit           | the container's terminal requires a password login                                                                                        |
| `NO_DOCKER_ROUTE_FOR_SERVER`              | operator limit           | a named server has no docker route and HTTP failed                                                                                        |
| `SSH_HOST_KEY_MISMATCH`                   | fixed rule               | the SSH host key does not match the known key                                                                                             |
| `NAME_CONFLICT`                           | operator limit           | `iris_add_server` names an existing Server Manager profile                                                                                |
| `READ_ONLY_STRICT`                        | operator limit           | a scratch-class tool is called under `read_only = "strict"`                                                                               |
| `BODY_NOT_REDACTABLE`                     | operator limit           | content under `redact` that iad cannot redact                                                                                             |
| `ELICITATION_NOT_FOUND`                   | not a refusal            | a resume names no pending entry for this tool and server                                                                                  |
| `BPL_TOO_DEEP`                            | not a refusal            | BPL nesting exceeds the parser's depth limit                                                                                              |
| `STORED_SOURCE_UNREADABLE`                | fixed rule               | a document to be compiled exists but its stored source cannot be read                                                                     |
| `COMPILE_SUPERCLASS_STALE`                | fixed rule               | a compile would also build an out-of-date superclass outside the library packages (names it)                                              |
| `HOST_PATH_BLOCKED`                       | fixed rule               | a tool that reads a host path was given one outside the project directory, with `..`, or of a kind it does not take (`reason` says which) |
| `SUPERCLASS_CHECK_FAILED`                 | not a refusal            | the superclass walk before a compile could not run or did not finish (fails closed)                                                       |
| `ELICITATION_SERVER_MISMATCH`             | not a refusal            | a resume names a different server than its pending entry                                                                                  |
| `SERVICE_ACCOUNT_TERMINAL_DENIED`         | operator limit           | the docker/SSH terminal ends the session after the drop: the service account lacks `%Development:USE`                                     |
| `JOURNAL_SEARCH_TIMEOUT`                  | not a refusal            | `journal_search` ran past the HTTP timeout; names the journal file and size                                                               |
| `INVALID_MANIFEST`                        | not a refusal            | a manifest or lock name contains control characters                                                                                       |
