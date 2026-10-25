---
name: iris-cpf-merge
tier: extra
description: >
  Use when configuring IRIS containers via CPF merge files (ISC_CPF_MERGE_FILE).
  Covers CPFPreset patterns, the grongierisc template pattern, how to set a
  password at first boot (and why a merge line cannot do it safely), and common
  CPF merge mistakes.
  Load when: setting up IRIS Docker containers, debugging ChangePassword errors,
  or customizing IRIS startup configuration without docker exec.
tags: [iris, cpf, docker, password, configuration, container, devtester]
---

# IRIS CPF Merge

**What it is**: A `merge.cpf` file processed by IRIS at startup (before the superserver opens) via the `ISC_CPF_MERGE_FILE` environment variable. Enables zero-`docker exec` configuration of users, services, memory, and namespaces.

---

## HARD GATE

- Set `ChangePassword=0` or `PasswordNeverExpires=1` only on an account whose password you have already replaced. On a fresh container the password is the published default `SYS`, and clearing its expiry makes `SYS` work, for good, for anyone who can reach the container.
- A merge line cannot set a generated password. Set it with `-e IRIS_PASSWORD` or with step 4 of the `iris-agentic-dev-setup` skill (see "Setting the \_SYSTEM password" below).
- CPF merge fires **before** IRIS opens the superserver — this is a feature, not a limitation
- Publish container ports on `127.0.0.1` only (`-p 127.0.0.1:<host>:<container>`), the same as every other setup path.
- Ask before running a merge file against a container that already exists: it is someone's work, and the merge runs at first boot only.

---

## Standard Presets (iris-devtester CPFPreset)

```python
from iris_devtester.config.presets import CPFPreset

# Enable CallIn service only
CPFPreset.ENABLE_CALLIN
# → [Actions]
# → ModifyService:Name=%Service_CallIn,Enabled=1,AutheEnabled=48

# Clear the expiry on SuperUser and _SYSTEM, enable CallIn
CPFPreset.SECURE_DEFAULTS
# Despite the name this keeps the default password: the account stays `SYS`, with no expiry.
# Use it for CI containers that are never published or shared, and nowhere else.

# Memory tuning for CI
CPFPreset.CI_OPTIMIZED
# → [config]
# → globals=0,0,256,0,0,0
# → gmheap=64000
```

---

## grongierisc Template Pattern

From [iris-fhir-facade-and-repo-template](https://github.com/grongierisc/iris-fhir-facade-and-repo-template):

```ini
# merge.cpf
[Actions]
CreateDatabase:Name=MYAPP_DATA,Directory=/dur/iris/mgr/MYAPP_DATA
CreateNamespace:Name=MYAPP,Globals=MYAPP_DATA,Routines=MYAPP_DATA,Interop=1
ModifyService:Name=%Service_CallIn,Enabled=1,AutheEnabled=48
```

```yaml
# docker-compose.yml
services:
  iris:
    ports:
      - "127.0.0.1:1972:1972"
      - "127.0.0.1:52773:52773"
    environment:
      - ISC_CPF_MERGE_FILE=/irisdev/app/merge.cpf
      - ISC_DATA_DIRECTORY=/dur/iris
      # Passed through from the shell that runs `docker compose up`; never a literal here.
      - IRIS_PASSWORD
    volumes:
      - .:/irisdev/app
      - iris-data:/dur
```

---

## CPF Actions Reference

```ini
[Actions]
# Create a namespace with separate databases
CreateResource:Name=%DB_MYNS_DATA,Description="MYNS data"
CreateDatabase:Name=MYNS_DATA,Directory=/dur/iris/mgr/MYNS_DATA
CreateNamespace:Name=MYNS,Globals=MYNS_DATA,Routines=MYNS_DATA,Interop=1

# Enable CallIn (required for DBAPI)
ModifyService:Name=%Service_CallIn,Enabled=1,AutheEnabled=48

# Give an application user only the roles it needs. %All is for an administrator.
ModifyUser:Name=myapp,Roles=%Developer
```

---

## Setting the \_SYSTEM password

A merge file is the wrong place for a password you generated. Measured on `intersystemsdc/iris-community:2026.2`: `PasswordHash=` lines built from a generated password (PBKDF2 hashes, hex and base64, with and without the algorithm and work-factor fields) either were ignored or left `_SYSTEM` with a password nobody knew. `Security.Users` has no `GeneratePasswordHash` method, and the hash that `CPFPreset.SECURE_DEFAULTS` ships is the hash of `SYS`. Do not copy it.

Two ways that do work, both measured on a fresh container:

```bash
# 1. The intersystemsdc images read IRIS_PASSWORD at first boot and set _SYSTEM's password.
#    It then shows in `docker inspect` and in the run command, so prefer 2 for anything shared.
docker run -d --name my-iris -p 127.0.0.1:1972:1972 -p 127.0.0.1:52773:52773 \
  -e IRIS_PASSWORD="$IRIS_PASSWORD" intersystemsdc/iris-community:latest
```

1. Generate a password on the host and send the change on `docker exec -i` stdin, as step 4 of the `iris-agentic-dev-setup` skill does. The password never reaches a command line or a file.

Either way the merge file keeps to services, namespaces and databases, and every other predefined account stays expired.

---

## iris-devtester Usage

```python
from iris_devtester import IRISContainer
from iris_devtester.config.presets import CPFPreset

# Automatic (1.18.0+): start() injects SECURE_DEFAULTS automatically
with IRISContainer.community() as iris:
    conn = iris.get_connection()   # expiry cleared by CPF, password still the default: CI only

# Manual override: provide your own CPF content
iris = (
    IRISContainer.community()
    .with_cpf_merge(CPFPreset.SECURE_DEFAULTS + "\nCreateResource:Name=%DB_APP,Description=app")
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

Do not patch only `SuperUser` and call it done: with the default password, `_SYSTEM` stays blocked until it has its own new password, and that is a password change, not an expiry flag. Do not patch both accounts' expiry either. That is what `SECURE_DEFAULTS` does, and it leaves two `%All` accounts open with the published password.
