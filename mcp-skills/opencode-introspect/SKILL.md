---
name: opencode-introspect
description: >
  Read and search opencode session logs from the SQLite DB at
  ~/.local/share/opencode/opencode.db. Use when you need to review what a
  session did, assess whether AI work was safe/correct, diagnose a "session
  wipe", find a specific tool call or output from a past session, or recover
  the DB after a migration reset.
---

# OpenCode Session Introspection

## Architecture (read this first)

OpenCode stores data in **two parallel stores**:

| Store | Path | Role |
|-------|------|------|
| SQLite DB | `~/.local/share/opencode/opencode.db` | Query cache — can be wiped and rebuilt |
| Flat JSON files | `~/.local/share/opencode/storage/` | **Ground truth** — never wiped by opencode |

The DB schema has these key tables: `project`, `session`, `message`, `part`.
- `session.directory` — working directory the session ran in
- `message.session_id` → FK to session
- `part.message_id` → FK to message; `part.data` is a **JSON blob** (not columns)

### part.data JSON structure

```
type: "text"       → d['text']                            (assistant/user prose)
type: "tool"       → d['tool'], d['state']['input'], d['state']['output']
type: "step-start" → d['snapshot']                        (context snapshot hash)
type: "step-finish"→ d['cost'], d['tokens']               (billing info)
type: "patch"      → file diff applied by agent
```

For bash tool calls: `d['state']['input']['command']`, `d['state']['output']`
For read/write/edit: `d['state']['input']['path']` or `d['state']['input']['filePath']`

**NEVER use** `json_extract(p.data, '$.input')` at the SQLite level — it's nested under `state`. Use Python to parse.

---

## 1. List recent sessions

```python
import sqlite3, json
db = sqlite3.connect('/Users/tdyar/.local/share/opencode/opencode.db')

rows = db.execute("""
    SELECT s.id, s.title, s.directory,
           s.parent_id,
           datetime(s.time_created/1000,'unixepoch','localtime') as created,
           datetime(s.time_updated/1000,'unixepoch','localtime') as updated
    FROM session s
    ORDER BY s.time_updated DESC
    LIMIT 20
""").fetchall()

for r in rows:
    parent = f" [child of {r[3][:20]}]" if r[3] else ""
    print(f"{r[4]} – {r[5]}")
    print(f"  {r[0][:30]}{parent}")
    print(f"  {r[1][:80]}")
    print(f"  dir: {r[2]}")
    print()
```

## 2. List sessions by project directory

```python
rows = db.execute("""
    SELECT s.id, s.title, s.parent_id,
           datetime(s.time_created/1000,'unixepoch','localtime') as created,
           datetime(s.time_updated/1000,'unixepoch','localtime') as updated
    FROM session s
    WHERE s.directory LIKE '%/arno%'   -- change to your project
    ORDER BY s.time_updated DESC
    LIMIT 20
""").fetchall()
```

## 3. Read a session's full activity log

```python
import sqlite3, json

def read_session(session_id):
    db = sqlite3.connect('/Users/tdyar/.local/share/opencode/opencode.db')
    rows = db.execute("""
        SELECT p.data
        FROM part p JOIN message m ON p.message_id = m.id
        WHERE m.session_id = ?
        ORDER BY p.rowid
    """, (session_id,)).fetchall()

    for (data_str,) in rows:
        try:
            d = json.loads(data_str)
        except:
            continue
        t = d.get('type', '')
        if t == 'text' and d.get('text', '').strip():
            print(f"[TEXT] {d['text'][:300]}")
        elif t == 'tool':
            tool = d.get('tool', '')
            state = d.get('state', {})
            inp = state.get('input', {})
            cmd = inp.get('command','') or inp.get('path','') or inp.get('filePath','') or str(inp)[:80] if isinstance(inp, dict) else str(inp)[:80]
            out = (state.get('output', '') or '')[:150]
            print(f"[{tool}] {cmd[:150]}")
            if out:
                print(f"  → {out}")
        elif t == 'step-finish':
            cost = d.get('cost')
            if cost:
                print(f"[cost ${cost:.4f}]")
```

## 4. Search across sessions for a keyword

```python
import sqlite3, json, re

def search_sessions(keyword, project_dir=None):
    db = sqlite3.connect('/Users/tdyar/.local/share/opencode/opencode.db')
    where = "WHERE s.directory LIKE ?" if project_dir else "WHERE 1=1"
    params = (f'%{project_dir}%',) if project_dir else ()

    rows = db.execute(f"""
        SELECT p.data, s.id, s.title, s.directory
        FROM part p
        JOIN message m ON p.message_id = m.id
        JOIN session s ON m.session_id = s.id
        {where}
        ORDER BY p.rowid
    """, params).fetchall()

    for (data_str, sess_id, title, directory) in rows:
        try:
            d = json.loads(data_str)
        except:
            continue
        txt = ''
        if d.get('type') == 'text':
            txt = d.get('text', '')
        elif d.get('type') == 'tool':
            state = d.get('state', {})
            inp = state.get('input', {})
            txt = inp.get('command','') if isinstance(inp, dict) else str(inp)
            txt += '\n' + (state.get('output','') or '')
        if re.search(keyword, txt, re.IGNORECASE):
            print(f"\n[{sess_id[:25]}] {title[:60]}")
            print(f"  dir: {directory}")
            print(f"  match: {txt[:300]}")
```

## 5. Count sessions and check DB health

```bash
sqlite3 ~/.local/share/opencode/opencode.db \
  "SELECT COUNT(*) as sessions FROM session;
   SELECT COUNT(*) as messages FROM message;
   SELECT COUNT(*) as parts FROM part;"
```

If any count is 0 → DB was wiped. Sessions are still on disk (see recovery below).

---

## DB Wipe Recovery

The DB is a **cache** — flat files are the ground truth.

### When does a wipe happen?
Running `opencode debug config` (or any command that spawns a child opencode process) against a DB from an older schema version triggers Drizzle migration, which drops and recreates tables. The flat files at `~/.local/share/opencode/storage/` are never touched.

### Recovery procedure

```python
import json, sqlite3, os, glob

db_path = os.path.expanduser("~/.local/share/opencode/opencode.db")
conn = sqlite3.connect(db_path)

# Step 1: Restore projects
storage_project = os.path.expanduser("~/.local/share/opencode/storage/project")
inserted_projects = 0
for f in glob.glob(f"{storage_project}/*.json"):
    try:
        d = json.load(open(f))
        conn.execute("""
            INSERT OR IGNORE INTO project (id, worktree, vcs, name, time_created, time_updated, sandboxes)
            VALUES (?, ?, ?, ?, ?, ?, ?)
        """, (d['id'], d.get('worktree',''), d.get('vcs'), d.get('name'),
              d.get('time',{}).get('created',0), d.get('time',{}).get('updated',0),
              json.dumps(d.get('sandboxes',[]))))
        inserted_projects += 1
    except Exception as e:
        pass
conn.commit()
print(f"Projects: {inserted_projects}")

# Step 2: Restore sessions
storage_session = os.path.expanduser("~/.local/share/opencode/storage/session")
inserted_sessions = skipped = 0
for project_hash in os.listdir(storage_session):
    project_dir = os.path.join(storage_session, project_hash)
    if not os.path.isdir(project_dir):
        continue
    for f in glob.glob(f"{project_dir}/*.json"):
        try:
            d = json.load(open(f))
            conn.execute("""
                INSERT OR IGNORE INTO session
                  (id, project_id, parent_id, slug, directory, title, version, time_created, time_updated, permission)
                VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?)
            """, (d['id'], project_hash, d.get('parentID'), d.get('slug',''),
                  d.get('path', d.get('directory','')), d.get('title',''),
                  d.get('version',''), d.get('time',{}).get('created',0),
                  d.get('time',{}).get('updated',0), d.get('permission')))
            inserted_sessions += 1
        except Exception as e:
            skipped += 1
conn.commit()
print(f"Sessions: {inserted_sessions} inserted, {skipped} skipped")

# Step 3: Restore messages
storage_message = os.path.expanduser("~/.local/share/opencode/storage/message")
inserted_msgs = 0
for session_dir in os.listdir(storage_message):
    msg_dir = os.path.join(storage_message, session_dir)
    if not os.path.isdir(msg_dir):
        continue
    for f in glob.glob(f"{msg_dir}/*.json"):
        try:
            d = json.load(open(f))
            conn.execute("""
                INSERT OR IGNORE INTO message (id, session_id, time_created, time_updated, data)
                VALUES (?, ?, ?, ?, ?)
            """, (d['id'], d.get('sessionID', session_dir),
                  d.get('time',{}).get('created',0), d.get('time',{}).get('updated',0),
                  json.dumps(d)))
            inserted_msgs += 1
            if inserted_msgs % 50000 == 0:
                conn.commit()
                print(f"  {inserted_msgs} messages...")
        except Exception:
            pass
conn.commit()
print(f"Messages: {inserted_msgs}")
```

After recovery: reopen opencode normally — it will use the restored DB.

---

## Key facts

- `~/.local/share/opencode/storage/session/<project-hash>/` — one JSON file per session
- Project hash = SHA1 of the project's git worktree path (deterministic)
- The DB's `part` table is populated from `storage/message/` JSON files
- `opencode debug config` is SAFE to run — it prints config, does NOT wipe sessions
  - The wipe was caused by a **schema migration** triggered by running a child opencode process against a DB with an older schema version
- The WAL files (`opencode.db-wal`, `opencode.db-shm`) are part of the active DB transaction journal — back up all three together
