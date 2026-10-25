//! `iris-agentic-dev trust`: the only writer of trust entries (spec 141 FR-036, R4a D3).
//!
//! Granting trust needs a terminal on stdin and stderr and the typed word `trust`. `--show`,
//! `--list` and `--revoke` need none: showing and revoking can only tighten. No MCP tool reaches
//! this; an agent that wants a file trusted can run `--show` and ask the operator to run the real
//! command.
//!
//! A terminal check proves a terminal, not a person. An agent whose shell runs commands in a
//! pseudo-terminal passes it. The boundary that holds is the operator approving that shell command.

use anyhow::{anyhow, Context, Result};
use clap::Args;
use iris_agentic_dev_core::iris::config_sources::{
    loosening_settings, operator_only_items, trust_entries, trust_state, ConfigSnapshot,
    LoadInputs, TrustEntry, TrustState, CONFIG_FILE_NAME, TRUST_FORMAT,
};
use std::io::{BufRead, IsTerminal, Write};
use std::path::{Path, PathBuf};
use toml_edit::{value, Array, ArrayOfTables, DocumentMut, Item, Table, Value};

#[derive(Args)]
pub struct TrustCommand {
    /// The workspace file to trust. Defaults to the one iad would load from this directory.
    pub path: Option<PathBuf>,
    /// Print what trusting would apply and write nothing.
    #[arg(long)]
    pub show: bool,
    /// List the trust entries in the operator file and whether each still matches.
    #[arg(long)]
    pub list: bool,
    /// Remove the trust entry for this path.
    #[arg(long, value_name = "PATH")]
    pub revoke: Option<PathBuf>,
}

/// Exit codes of contracts/trust-cli.md (0 is a normal return).
const DECLINED: i32 = 1;
const REFUSED: i32 = 2;

fn operator_file() -> Result<PathBuf> {
    let home = dirs::home_dir().ok_or_else(|| anyhow!("no home directory"))?;
    Ok(home.join(CONFIG_FILE_NAME))
}

fn canon(p: &Path) -> PathBuf {
    std::fs::canonicalize(p).unwrap_or_else(|_| p.to_path_buf())
}

fn refuse(msg: impl std::fmt::Display) -> ! {
    eprintln!("error: {msg}");
    crate::exit(REFUSED)
}

impl TrustCommand {
    pub async fn run(self) -> Result<()> {
        if self.list {
            return list();
        }
        if let Some(p) = &self.revoke {
            return revoke(p);
        }
        trust(self.path.as_deref(), self.show)
    }
}

/// The file `trust` means when no path is given: the workspace file iad would load from here.
fn default_target() -> Option<PathBuf> {
    let snap = ConfigSnapshot::load(&LoadInputs::from_process(None, None));
    snap.workspace_layer.map(|f| f.path)
}

fn trust(path: Option<&Path>, show_only: bool) -> Result<()> {
    let target = match path {
        Some(p) => p.to_path_buf(),
        None => default_target().unwrap_or_else(|| {
            refuse("no workspace file found from this directory; name the file: iris-agentic-dev trust <path>")
        }),
    };
    let op_path = operator_file()?;
    if target.exists() && op_path.exists() && canon(&target) == canon(&op_path) {
        refuse(format!(
            "{} is the operator file, which is always trusted; there is nothing to trust",
            op_path.display()
        ));
    }
    let contents = std::fs::read_to_string(&target)
        .with_context(|| format!("cannot read {}", target.display()))
        .unwrap_or_else(|e| refuse(e));
    let table: toml::Table = match contents.parse() {
        Ok(t) => t,
        Err(e) => {
            let (line, col) = e
                .span()
                .map(|s| {
                    let before = &contents[..s.start.min(contents.len())];
                    (
                        before.matches('\n').count() + 1,
                        before.rsplit('\n').next().map_or(0, |l| l.chars().count()) + 1,
                    )
                })
                .unwrap_or((0, 0));
            refuse(format!(
                "{} does not parse (line {line}, column {col}): {}",
                target.display(),
                e.message()
            ))
        }
    };
    let target = canon(&target);
    let settings = loosening_settings(&table);
    if settings.is_empty() {
        println!("{} loosens nothing; trust is not needed.", target.display());
        if show_only {
            print_remedies(&op_path);
        }
        return Ok(());
    }

    print_effects(&target, &table, &settings);
    if show_only {
        print_remedies(&op_path);
        return Ok(());
    }

    if !(std::io::stdin().is_terminal() && std::io::stderr().is_terminal()) {
        refuse(format!(
            "trust needs an interactive terminal; run `iris-agentic-dev trust {}` yourself in a shell",
            target.display()
        ));
    }
    eprint!("Type \"trust\" to trust this file: ");
    std::io::stderr().flush().ok();
    let mut line = String::new();
    std::io::stdin().lock().read_line(&mut line)?;
    if line.trim() != "trust" {
        eprintln!("Not trusted; nothing was written.");
        crate::exit(DECLINED);
    }

    let entry = TrustEntry {
        path: target.to_string_lossy().into_owned(),
        format: TRUST_FORMAT,
        settings,
    };
    let src = std::fs::read_to_string(&op_path).unwrap_or_default();
    let edited = edit_operator_file(&src, &entry, &now_rfc3339())
        .with_context(|| format!("cannot update {}", op_path.display()))
        .unwrap_or_else(|e| refuse(format!("{e:#}")));
    write_private_atomic(&op_path, edited.as_bytes())?;
    println!("Trusted {}", entry.path);
    // A trust entry loosens, so a running server holds it back until its next start (FR-043).
    println!("Restart iris-agentic-dev (the MCP server) to apply it; a running server holds the new entry back.");
    Ok(())
}

/// What the operator changes to lift each limit a refusal can name. Refusals returned to the agent
/// carry the setting but not this (FR-039); it goes here and to stderr.
fn print_remedies(operator_file: &Path) {
    println!("Limits a call can be refused by, and the edit that lifts each:");
    println!();
    for line in iris_agentic_dev_core::policy::refusal::remedy_catalogue(Some(operator_file)) {
        println!("  {line}");
    }
    println!();
    println!("An edit that loosens applies after iris-agentic-dev restarts; one that tightens applies at once.");
}

fn print_effects(target: &Path, table: &toml::Table, settings: &[String]) {
    println!("Workspace file: {}", target.display());
    println!();
    println!("Trusting it applies these settings:");
    println!();
    let width = settings.iter().map(|s| s.len()).max().unwrap_or(0);
    for s in settings {
        println!("  {s:<width$}  {}", effect_of(s));
    }
    println!();
    let only = operator_only_items(table, target);
    if !only.is_empty() {
        println!("It also sets things only your own file can set. Trust does not apply them:");
        println!();
        for i in &only {
            println!("  {}", i.setting);
        }
        println!();
    }
}

/// A short plain description of what a canonical setting line does once trusted.
fn effect_of(line: &str) -> String {
    let key = line.split(" = ").next().unwrap_or(line);
    let parts: Vec<&str> = key.split('.').collect();
    let server = |i: usize| parts.get(i).copied().unwrap_or("default");
    match key {
        "write_tools_enabled" => "default server: write tools on".into(),
        "destructive_tools_enabled" => "default server: destructive tools on".into(),
        "compile_time_gate" => "default server: compile-time code execution gate off".into(),
        "tls_verify" => "certificates are not validated".into(),
        "ssh_host" => "docker commands run on that SSH host".into(),
        "service_username" => "agent code runs as that service account".into(),
        "connection" => "the server the settings here apply to".into(),
        "connection.namespace" | "connection.container" => "where on that server they apply".into(),
        k if k.starts_with("policy.") => {
            let what = match parts.last().copied().unwrap_or("") {
                "writeTools" => "write tools on",
                "destructiveTools" => "destructive tools on",
                "compileTimeGate" => "compile-time code execution gate off",
                "mcpTemplate" => "environment template loosened",
                "allow" => "tool categories allowed",
                "dataPolicy" => "bulk-data tools allowed",
                "dataPolicyKillAllowlist" => "kills exempt from custom blocklist patterns",
                _ => "policy loosened",
            };
            format!("server {}: {what}", server(1))
        }
        k if k.starts_with("instance.") => {
            let what = match parts.last().copied().unwrap_or("") {
                "address" | "container" | "namespace" => "the server these settings apply to",
                "role" => "role is workbench (writes are not confirmed)",
                "service_username" => "agent code runs as that service account",
                "ssh_host" => "docker commands run on that SSH host",
                "tls_verify" => "certificates are not validated",
                _ => "instance setting loosened",
            };
            format!("server {}: {what}", server(1))
        }
        _ => String::new(),
    }
}

fn list() -> Result<()> {
    let op_path = operator_file()?;
    let src = std::fs::read_to_string(&op_path).unwrap_or_default();
    let table: toml::Table = src
        .parse()
        .map_err(|e: toml::de::Error| {
            anyhow!("{} does not parse: {}", op_path.display(), e.message())
        })
        .unwrap_or_else(|e| refuse(e));
    let entries = trust_entries(&table);
    if entries.is_empty() {
        println!("No trust entries in {}.", op_path.display());
        return Ok(());
    }
    for e in &entries {
        let p = Path::new(&e.path);
        let state = std::fs::read_to_string(p)
            .ok()
            .and_then(|c| c.parse::<toml::Table>().ok())
            .map(|t| trust_state(p, &t, &entries));
        match state {
            None => println!("missing\t{}", e.path),
            Some(TrustState::Trusted) => println!("trusted\t{}", e.path),
            Some(TrustState::NotNeeded) => {
                println!("trusted\t{}\t(it loosens nothing now)", e.path)
            }
            Some(TrustState::Lapsed { changed }) => {
                println!("lapsed\t{}", e.path);
                for c in changed {
                    println!(
                        "\t{}: {} -> {}",
                        c.setting,
                        c.was.as_deref().unwrap_or("(absent)"),
                        c.now.as_deref().unwrap_or("(absent)")
                    );
                }
            }
            Some(TrustState::Untrusted) => println!("lapsed\t{}", e.path),
        }
    }
    Ok(())
}

fn revoke(path: &Path) -> Result<()> {
    let op_path = operator_file()?;
    let src = std::fs::read_to_string(&op_path).unwrap_or_default();
    // The entry is keyed by the canonical path trust wrote; try the path as given too, so an entry
    // for a file that no longer exists can still be removed.
    let canonical = canon(path).to_string_lossy().into_owned();
    let given = path.to_string_lossy().into_owned();
    let (mut out, mut found) =
        revoke_entry(&src, &canonical).unwrap_or_else(|e| refuse(format!("{e:#}")));
    if canonical != given {
        let (o2, f2) = revoke_entry(&out, &given).unwrap_or_else(|e| refuse(format!("{e:#}")));
        out = o2;
        found |= f2;
    }
    if !found {
        println!("No trust entry for {}.", path.display());
        return Ok(());
    }
    write_private_atomic(&op_path, out.as_bytes())?;
    println!("Revoked trust for {}.", path.display());
    Ok(())
}

/// `src` (the operator file's text) with the entry for `entry.path` replaced by `entry`, comments
/// and order elsewhere untouched.
pub fn edit_operator_file(src: &str, entry: &TrustEntry, trusted_at: &str) -> Result<String> {
    let mut doc: DocumentMut = src
        .parse()
        .map_err(|e| anyhow!("the operator file does not parse: {e}"))?;
    drop_entries(&mut doc, &entry.path)?;

    let mut settings = Array::new();
    for s in &entry.settings {
        // A literal string reads best in the file: the lines hold double quotes.
        let mut v = if s.contains('\'') || s.contains('\n') {
            Value::from(s.as_str())
        } else {
            format!("'{s}'")
                .parse::<Value>()
                .unwrap_or_else(|_| Value::from(s.as_str()))
        };
        v.decor_mut().set_prefix("\n  ");
        settings.push_formatted(v);
    }
    settings.set_trailing("\n");
    settings.set_trailing_comma(true);

    let mut t = Table::new();
    t.insert("path", value(entry.path.clone()));
    t.insert("format", value(i64::from(entry.format)));
    t.insert("settings", Item::Value(Value::Array(settings)));
    t.insert("trusted_at", value(trusted_at));

    let item = doc
        .entry("trust")
        .or_insert_with(|| Item::ArrayOfTables(ArrayOfTables::new()));
    item.as_array_of_tables_mut()
        .ok_or_else(|| anyhow!("`trust` in the operator file is not an array of [[trust]] tables"))?
        .push(t);
    Ok(doc.to_string())
}

/// `src` without the entry for `path`, and whether there was one.
pub fn revoke_entry(src: &str, path: &str) -> Result<(String, bool)> {
    let mut doc: DocumentMut = src
        .parse()
        .map_err(|e| anyhow!("the operator file does not parse: {e}"))?;
    let removed = drop_entries(&mut doc, path)?;
    if !removed {
        return Ok((src.to_string(), false));
    }
    Ok((doc.to_string(), true))
}

/// Remove every `[[trust]]` entry whose `path` is `path`. An emptied array goes too.
fn drop_entries(doc: &mut DocumentMut, path: &str) -> Result<bool> {
    let Some(item) = doc.get_mut("trust") else {
        return Ok(false);
    };
    let arr = item.as_array_of_tables_mut().ok_or_else(|| {
        anyhow!("`trust` in the operator file is not an array of [[trust]] tables")
    })?;
    let before = arr.len();
    arr.retain(|t| t.get("path").and_then(|p| p.as_str()) != Some(path));
    let removed = arr.len() != before;
    if arr.is_empty() {
        doc.remove("trust");
    }
    Ok(removed)
}

/// Write `bytes` to `path` through a temporary file in the same directory, owner-only, then rename
/// over the original. Through a symlink, the link's target is replaced and the link stays.
fn write_private_atomic(path: &Path, bytes: &[u8]) -> Result<()> {
    let target = if path.is_symlink() {
        canon(path)
    } else {
        path.to_path_buf()
    };
    let dir = target.parent().unwrap_or_else(|| Path::new("."));
    std::fs::create_dir_all(dir)?;
    let tmp = dir.join(format!(".{CONFIG_FILE_NAME}.tmp{}", std::process::id()));
    {
        let mut opts = std::fs::OpenOptions::new();
        opts.write(true).create(true).truncate(true);
        #[cfg(unix)]
        {
            use std::os::unix::fs::OpenOptionsExt;
            opts.mode(0o600);
        }
        let mut f = opts
            .open(&tmp)
            .with_context(|| format!("cannot write {}", tmp.display()))?;
        f.write_all(bytes)?;
        f.sync_all().ok();
    }
    std::fs::rename(&tmp, &target).inspect_err(|_| {
        let _ = std::fs::remove_file(&tmp);
    })?;
    Ok(())
}

/// `2026-10-23T14:02:11Z`, from the system clock.
fn now_rfc3339() -> String {
    let secs = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0) as i64;
    let (days, rem) = (secs.div_euclid(86_400), secs.rem_euclid(86_400));
    // Civil date from a day count (Howard Hinnant's algorithm).
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z.rem_euclid(146_097);
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let y = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    let y = if m <= 2 { y + 1 } else { y };
    format!(
        "{y:04}-{m:02}-{d:02}T{:02}:{:02}:{:02}Z",
        rem / 3600,
        rem % 3600 / 60,
        rem % 60
    )
}

#[cfg(test)]
mod tests {
    use super::now_rfc3339;

    #[test]
    fn the_timestamp_is_rfc3339_utc() {
        let t = now_rfc3339();
        assert_eq!(t.len(), 20, "{t}");
        assert!(t.ends_with('Z') && &t[10..11] == "T", "{t}");
        assert!(t.starts_with("20"), "{t}");
    }
}
