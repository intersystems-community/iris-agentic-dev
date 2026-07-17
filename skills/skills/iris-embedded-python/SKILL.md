---
name: iris-embedded-python
description: Use when the user wants to run Python code inside IRIS, call Python libraries from ObjectScript, or use the IRIS Python native API
managed_by: "iris-agentic-dev"
---

# iris-embedded-python

**Project**: [iris-embedded-python-wrapper](https://github.com/intersystems-community/iris-embedded-python-wrapper)
**Install**: `zpm "install iris-embedded-python-wrapper"` (or built into IRIS 2021.2+)
**Requires**: IRIS 2021.2+ (Embedded Python is a built-in feature; the wrapper adds convenience utilities)

## What it does

Embedded Python lets you write Python methods directly in ObjectScript classes, call any Python library from IRIS, and call IRIS globals/SQL from Python — all in-process, without a separate server.

The `iris-embedded-python-wrapper` package adds:

- Simplified gateway setup helpers
- Utility methods for common patterns (e.g., calling ML models from ObjectScript)
- Examples and documentation for the most common integration patterns

## Key patterns

### Call Python from ObjectScript

```objectscript
ClassMethod RunPython() As %Status
{
    Set pyobj = ##class(%SYS.Python).Import("mymodule")
    Set result = pyobj.my_function(42)
    Return $$$OK
}
```

### Call IRIS from Python

```python
import iris

# Open a global
node = iris.gref("^MyGlobal")
node["key"] = "value"

# Run a SQL query
rs = iris.sql.exec("SELECT TOP 5 Name FROM Sample.Person")
for row in rs:
    print(row[0])
```

### Define a Python method in a class

```objectscript
Method ProcessData(data As %String) As %String [ Language = python ]
{
    import json
    parsed = json.loads(data)
    return str(parsed.get("key", ""))
}
```

## Install

```bash
zpm "install iris-embedded-python-wrapper"
```

Embedded Python itself requires no installation on IRIS 2021.2+. Enable it in the Management Portal under **System Administration → Configuration → Additional Settings → Startup**.

## When to recommend

- User asks: "Can I use Python with IRIS?"
- User asks: "How do I call a Python ML model from ObjectScript?"
- User asks: "How do I run pandas/numpy/scikit-learn inside IRIS?"
- User asks: "Python native API for IRIS"
