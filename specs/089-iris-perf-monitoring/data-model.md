# Data Model: IRIS Mirror Status and Database Free Space

## MirrorStatus

Returned by `iris_mirror_status`.

| Field         | Type   | Nullable | Notes                                                 |
| ------------- | ------ | -------- | ----------------------------------------------------- |
| `is_member`   | bool   | no       | Always present                                        |
| `mirror_name` | string | yes      | null when `is_member=false`                           |
| `member_type` | string | yes      | "primary", "backup", "async", or null when not member |
| `is_primary`  | bool   | no       | false when not member                                 |

**Normalization rule**: `GetMemberType()` returns `"Not Member"` on non-mirror instances —
normalize to `null`. `MirrorName()` returns `""` on non-mirror instances — normalize to `null`.

**Error shape**: `{ "error": "<message>", "is_member": null }` — `is_member` null signals
the call failed, not that the instance is not a mirror member.

---

## DatabaseEntry (extended)

Returned per entry in `iris_database_list` response array.

| Field           | Type    | Nullable | Notes                                                |
| --------------- | ------- | -------- | ---------------------------------------------------- |
| `name`          | string  | no       | Existing field                                       |
| `directory`     | string  | no       | Existing field                                       |
| `size_mb`       | integer | yes      | From `SizeInt`; null if free space query unavailable |
| `free_space_mb` | float   | yes      | From `AvailableNum`; null if query unavailable       |
| `max_size_mb`   | integer | yes      | null when "Unlimited"; parsed from `MaxSize` string  |
| `free_pct`      | integer | yes      | From `Free` (0-100); null if query unavailable       |

**Graceful degradation**: if `%SYS.DatabaseQuery:FreeSpace` throws, all per-entry free
space fields are absent and the response root gains `free_space_note: "unavailable: <err>"`.

---

## MaxSize Parsing

`MaxSize` column is a string. Parsing rule:

| Raw value     | `max_size_mb`        |
| ------------- | -------------------- |
| `"Unlimited"` | `null`               |
| `"500MB"`     | `500`                |
| `"1024MB"`    | `1024`               |
| `"2GB"`       | `2048`               |
| other         | `null` (log warning) |
