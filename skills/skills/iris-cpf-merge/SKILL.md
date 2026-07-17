---
name: iris-cpf-merge
description: >
  Use when configuring IRIS containers via CPF merge files (ISC_CPF_MERGE_FILE).
  Covers CPFPreset patterns, the grongierisc template pattern for password
  pre-configuration, ChangePassword=0 via Actions, and common CPF merge mistakes.
  Load when: setting up IRIS Docker containers, debugging ChangePassword errors,
  or customizing IRIS startup configuration without docker exec.
tags: [iris, cpf, docker, password, configuration, container, devtester]
---

# IRIS CPF Merge

**What it is**: A `merge.cpf` file processed by IRIS at startup (before the superserver opens) via the `ISC_CPF_MERGE_FILE` environment variable. Enables zero-`docker exec` configuration of users, services, memory, and namespaces.

---

## HARD GATE

- `ChangePassword=0` must be set for **both** `_SYSTEM` and `SuperUser` — only patching one leaves the default connection user broken
- CPF merge fires **before** IRIS opens the superserver — this is a feature, not a limitation
- Do NOT rely on `unexpire_all_passwords()` via docker exec as the primary path — CPF merge is faster, more reliable, and works in restricted CI environments

---

## Standard Presets (iris-devtester CPFPreset)

```python
from iris_devtester.config.presets import CPFPreset

# Enable CallIn service only
CPFPreset.ENABLE_CALLIN
# → [Actions]
# → ModifyService:Name=%Service_CallIn,Enabled=1,AutheEnabled=48

# Clear ChangePassword for _SYSTEM + SuperUser, enable CallIn
CPFPreset.SECURE_DEFAULTS
# → [Actions]
# → ModifyService:Name=%Service_CallIn,Enabled=1,AutheEnabled=48
# → ModifyUser:Name=SuperUser,PasswordHash=<hash>,ChangePassword=0,PasswordNeverExpires=1
# → ModifyUser:Name=_SYSTEM,ChangePassword=0,PasswordNeverExpires=1

# Memory tuning for CI
CPFPreset.CI_OPTIMIZED
# → [config]
# → globals=0,0,256,0,0,0
# → gmheap=64000
```

---

## grongierisc Template Pattern

From [iris-fhir-facade-and-repo-template](https://github.com/grongierisc/iris-fhir-facade-and-repo-template):

```
# merge.cpf
[Actions]
CreateDatabase:Name=MYAPP_DATA,Directory=/dur/iris/mgr/MYAPP_DATA
CreateNamespace:Name=MYAPP,Globals=MYAPP_DATA,Routines=MYAPP_DATA,Interop=1
ModifyService:Name=%Service_CallIn,Enabled=1,AutheEnabled=48
ModifyUser:Name=SuperUser,ChangePassword=0,PasswordNeverExpires=1
ModifyUser:Name=_SYSTEM,ChangePassword=0,PasswordNeverExpires=1
```

```yaml
# docker-compose.yml
services:
  iris:
    environment:
      - ISC_CPF_MERGE_FILE=/irisdev/app/merge.cpf
      - ISC_DATA_DIRECTORY=/dur/iris
    volumes:
      - .:/irisdev/app
      - iris-data:/dur
```

---

## CPF Actions Reference

```
[Actions]
# Create a namespace with separate databases
CreateResource:Name=%DB_MYNS_DATA,Description="MYNS data"
CreateDatabase:Name=MYNS_DATA,Directory=/dur/iris/mgr/MYNS_DATA
CreateNamespace:Name=MYNS,Globals=MYNS_DATA,Routines=MYNS_DATA,Interop=1

# Enable CallIn (required for DBAPI)
ModifyService:Name=%Service_CallIn,Enabled=1,AutheEnabled=48

# Clear password expiration
ModifyUser:Name=_SYSTEM,ChangePassword=0,PasswordNeverExpires=1
ModifyUser:Name=SuperUser,ChangePassword=0,PasswordNeverExpires=1

# Custom user with known password hash
ModifyUser:Name=myapp,PasswordHash=<hash>,ChangePassword=0,PasswordNeverExpires=1,Roles=%ALL
```

---

## Generating a PasswordHash

```objectscript
// In IRIS terminal:
do ##class(Security.Users).GeneratePasswordHash("MyPassword", .hash, .salt)
write hash, ",", salt  // copy this into CPF
```

Or use the hash from `CPFPreset.SECURE_DEFAULTS` (`FBFE8593AEFA510C27FD184738D6E865A441DE98,u4ocm4qh`) which corresponds to password `SYS`.

---

## iris-devtester Usage

```python
from iris_devtester import IRISContainer
from iris_devtester.config.presets import CPFPreset

# Automatic (1.18.0+): start() injects SECURE_DEFAULTS automatically
with IRISContainer.community() as iris:
    conn = iris.get_connection()   # ChangePassword=0 already set by CPF

# Manual override: provide your own CPF content
iris = (
    IRISContainer.community()
    .with_cpf_merge(CPFPreset.SECURE_DEFAULTS + "\nModifyUser:Name=app,Roles=%ALL")
)

# From file
iris = IRISContainer.community().with_cpf_merge("/path/to/merge.cpf")
```

---

## Anti-Patterns

```python
# WRONG: calling unexpire_all_passwords() before every connection
unexpire_all_passwords(container_name)   # docker exec, fragile, fails in restricted CI
conn = iris.connect(...)

# RIGHT (1.18.0+): CPF merge handles it at startup
# iris.get_connection() is optimistic — no pre-emptive docker exec
conn = iris.get_connection()
```

```
# WRONG: only patching SuperUser
ModifyUser:Name=SuperUser,ChangePassword=0    # _SYSTEM still blocked!

# RIGHT: patch both
ModifyUser:Name=SuperUser,ChangePassword=0,PasswordNeverExpires=1
ModifyUser:Name=_SYSTEM,ChangePassword=0,PasswordNeverExpires=1
```
