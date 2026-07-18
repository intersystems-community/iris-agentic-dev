---
name: pyprod
description: Use when creating or modifying InterSystems IRIS interoperability production components in Python — Business Services, Business Processes, Business Operations, Adapters, Messages, or Production definitions.
metadata:
  version: "1.2.0"
  compatibility: iris, python, pyprod
references:
  - setup: references/setup.md
  - director: references/director.md
  - production-definition: references/production-definition.md
---

# Building with pyprod

**Read this skill once, then write all required files in one pass. Do not re-read
between writes.**

pyprod is the InterSystems Python library for IRIS interoperability productions.
Import from `intersystems_pyprod` — not `grongier.pex` or any other package.

Import only what your component needs — do not copy-paste the full import list into every file:

| Component           | Typical imports                                                              |
| ------------------- | ---------------------------------------------------------------------------- |
| Message             | `Column, JsonSerialize` (or `PickleSerialize`)                               |
| BusinessService     | `IRISParameter, IRISProperty, BusinessService, IRISLog, Status`              |
| BusinessProcess     | `IRISProperty, BusinessProcess, IRISLog, Status`                             |
| BusinessOperation   | `IRISParameter, IRISProperty, BusinessOperation, IRISLog, Status`            |

Set the IRIS package name at module level (applies to all classes in the file):

```python
iris_package_name = "MyPackage"
```

---

## Messages

Messages passed between Business Hosts **must** subclass `JsonSerialize` or
`PickleSerialize`. Use `Column()` for fields that should be SQL-queryable — a plain
Python attribute is not persisted as a separate column.

```python
class OrderMessage(JsonSerialize):
    order_id: str = Column(index=True)   # SQL column, indexed
    amount = Column(datatype=int)        # SQL column, integer
    note = "default"                     # NOT a Column — not SQL-queryable
```

`Column(default=None, datatype=None, description=None, index=False)` — string and
numeric types only.

---

## BusinessService

Receives input from an adapter (or direct call), packages it as a message, routes
forward.

```python
class MyService(BusinessService):

    ADAPTER = IRISParameter("MyPackage.MyAdapter")   # omit → adapterless (set pool_size=0)
    target = IRISProperty(
        settings="Target:selector?context={Ens.ContextSearch/ProductionItems?targets=1&productionName=@productionId}"
    )

    def on_process_input(self, input):
        request = OrderMessage(input)
        status, response = self.send_request_sync(self.target, request, timeout=-1)
        return status, response
```

`send_request_async` on BusinessService: `(target, request, description="")` — **no
`response_required`**.

---

## BusinessProcess

Orchestrates logic. **New instance per message — no persistent state.**

```python
class MyProcess(BusinessProcess):

    target = IRISProperty(
        settings="Target:selector?context={Ens.ContextSearch/ProductionItems?targets=1&productionName=@productionId}"
    )

    def on_request(self, request):
        # response_required=1 (integer, not True) — MUST also implement on_response below
        status = self.send_request_async(self.target, request, response_required=1)
        return status, None

    # ⚠ REQUIRED — omitting on_response causes NotImplementedError at runtime
    def on_response(self, request, response, call_request, call_response, completion_key):
        return Status.OK(), response
```

**Rule:** Every async dispatch with `response_required=1` **must** have a matching
`on_response` in the same class. No exceptions. If you only want fire-and-forget,
pass `response_required=0` and omit `on_response`. Never use `True`/`False` — integer
only.

For sync dispatch:

```python
status, response = self.send_request_sync(self.target, request, timeout=-1)
return status, response
```

---

## BusinessOperation

Receives typed requests, dispatches via `MessageMap`.

```python
class MyOperation(BusinessOperation):

    ADAPTER = IRISParameter("MyPackage.MyAdapter")   # optional
    connection_url = IRISProperty(default="http://localhost", description="Target URL")

    MessageMap = {
        "MyPackage.OrderMessage": "handle_order",   # key = iris_package_name.ClassName
        "MyPackage.CancelMessage": "handle_cancel",
    }

    def handle_order(self, request):
        IRISLog.Info(f"Processing order to {self.connection_url}")
        return Status.OK(), None

    def handle_cancel(self, request):
        return Status.OK(), None

    def on_message(self, request):   # optional fallback for unmatched types
        return Status.OK()
```

`send_request_async` on BusinessOperation: `(target, request, description="")` — **no
`response_required`**.

---

## IRISProperty vs IRISParameter

|         | `IRISProperty`                                                | `IRISParameter`                |
| ------- | ------------------------------------------------------------- | ------------------------------ |
| Purpose | Operator-configurable instance value (shows in production UI) | Class-level constant           |
| Mutable | Yes, per-instance                                             | No                             |
| Use for | URLs, targets, credentials, timeouts                          | Adapter class name (`ADAPTER`) |

```python
ADAPTER = IRISParameter("MyPackage.MyAdapter")   # constant — links adapter class
timeout = IRISProperty(default=30, description="Timeout in seconds")  # UI-editable
```

---

## Status and Logging

Every callback returns `Status` as its first element. Use a tuple return in all
message-handling methods:

```python
return Status.OK(), response      # handler: success, pass response back
return Status.OK(), None          # handler: success, no response to pass back
return Status.ERROR("message"), None   # handler: failure
```

`on_message` (BusinessOperation fallback only) may return bare `Status.OK()`.

```python
IRISLog.Info("message")
IRISLog.Warning("message")
IRISLog.Error("message")
```

---

## Common Mistakes

| Mistake                                    | Effect                                | Fix                                                          |
| ------------------------------------------ | ------------------------------------- | ------------------------------------------------------------ |
| Plain attribute instead of `Column()`      | Field not SQL-queryable               | Use `Column(datatype=...)`                                   |
| `response_required=True` (bool)            | Runtime error                         | Use integer `1`                                              |
| `send_request_async(response_required=1)` without `on_response` | `NotImplementedError` at runtime | Always implement `on_response` alongside the async call |
| `IRISParameter` for UI-editable value      | Not visible in production UI          | Use `IRISProperty`                                           |
| `IRISProperty` on BusinessProcess          | State lost (new instance per message) | Use only on adapters, services, operations                   |
| Wrong MessageMap key package               | Messages not dispatched               | Key must match `iris_package_name` of the **message** module |
| `pool_size=1` for adapterless service      | Hangs                                 | Use `pool_size=0`                                            |
| Import from `grongier.pex`                 | Wrong library                         | Import from `intersystems_pyprod`                            |

---

## Production Definition and Director

See [[pyprod-production-definition]] for the `Production` class and item types.
See [[pyprod-director]] for start/stop/inject via `Director`.
