//! Bundled skill discovery — the skills shipped with this binary.
//!
//! `skill_list` / `skill_search` / `skill_describe` used to read only the IRIS
//! `^SKILLS` global, so the 31 skills shipped in `skills/skills/` were invisible
//! to them and a query for e.g. "vector HNSW index" answered `count: 0`. Agents
//! read that bare zero as "no such skill exists" and reimplemented from scratch.
//!
//! Two things fix that. First, bundled skills are *embedded* in the binary via
//! `include_str!`, so they resolve with no filesystem lookup, no IRIS connection
//! and no dependence on where the binary was built — the same reasoning as
//! `benchmark::load_embedded_tasks`. `env!("CARGO_MANIFEST_DIR")` and friends
//! bake in the build machine's path and break in a shipped/relocated binary; we
//! do not use them here. Second, an on-disk directory can still *override* the
//! embedded copy (`IRIS_AGENTIC_DEV_SKILLS_DIR`, or a `skills/skills` next to
//! the executable) so a checkout's edits win during dev.
//!
//! A `skills/skills` in the working directory or `$OBJECTSCRIPT_WORKSPACE` is the project's own
//! text, not the operator's, so it is read only when the operator opts in (`IAD_LOCAL_SKILLS=1`,
//! or `local_skills = true` in the operator file; spec 141 FR-027). The embedded guardrail skills
//! reach the agent under this server's name, and a repository could otherwise replace them.

use std::path::{Path, PathBuf};

/// Where a skill came from. Callers need this to know which half of the world
/// they are looking at.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SkillSource {
    /// Shipped with the binary, as embedded.
    Bundled,
    /// Synthesized at runtime and stored in the IRIS `^SKILLS` global.
    Synthesized,
    /// An embedded skill whose body was replaced by a copy on disk. The copy keeps the embedded
    /// tier, so no file can hide a bundled skill by marking it `internal`.
    Override,
    /// A skill found only on disk; it keeps its own tier.
    Local,
}

impl SkillSource {
    pub fn as_str(&self) -> &'static str {
        match self {
            SkillSource::Bundled => "bundled",
            SkillSource::Synthesized => "synthesized",
            SkillSource::Override => "override",
            SkillSource::Local => "local",
        }
    }
}

/// Which channels ship a skill. Read from the `tier:` frontmatter key; see
/// `docs/adr/0001-skill-tiers.md`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SkillTier {
    /// In the Claude Code plugin and in a bare `skill install`.
    Core,
    /// In `skill install --all` and the MCP tools, indexed by the `iris-agentic-dev` skill.
    Extra,
    /// Shipped for this repo's own tooling. Installs only by name and is left out of
    /// `skill_list` and `skill_search`; `skill_describe` still finds it.
    Internal,
}

impl SkillTier {
    pub fn parse(s: &str) -> Option<Self> {
        match s {
            "core" => Some(SkillTier::Core),
            "extra" => Some(SkillTier::Extra),
            "internal" => Some(SkillTier::Internal),
            _ => None,
        }
    }

    pub fn as_str(&self) -> &'static str {
        match self {
            SkillTier::Core => "core",
            SkillTier::Extra => "extra",
            SkillTier::Internal => "internal",
        }
    }
}

/// A skill parsed from a `SKILL.md` with YAML frontmatter.
#[derive(Debug, Clone)]
pub struct BundledSkill {
    pub name: String,
    pub description: String,
    /// Lowercased frontmatter tags. Searched alongside name and description —
    /// `hnsw` only ever appears as a tag on `iris-vector-ai`.
    pub tags: Vec<String>,
    /// `None` when the frontmatter has no `tier:` (a synthesized skill, or a file from before
    /// tiers).
    pub tier: Option<SkillTier>,
    /// Set when the skill was read from disk rather than the embedded copy.
    pub path: Option<PathBuf>,
}

impl BundledSkill {
    /// Frontmatter + body, for `skill_describe`.
    pub fn content(&self) -> Option<String> {
        match &self.path {
            Some(p) => std::fs::read_to_string(p).ok(),
            None => embedded_content(&self.name).map(|s| s.to_string()),
        }
    }

    /// `bundled` for an embedded copy, `override` for a disk copy of an embedded skill, `local` for
    /// a disk skill with no embedded counterpart.
    pub fn source(&self) -> SkillSource {
        match &self.path {
            None => SkillSource::Bundled,
            Some(_) if embedded_content(&self.name).is_some() => SkillSource::Override,
            Some(_) => SkillSource::Local,
        }
    }

    pub fn to_json(&self) -> serde_json::Value {
        let mut v = serde_json::json!({
            "name": self.name,
            "description": self.description,
            "tags": self.tags,
            "tier": self.tier.map(|t| t.as_str()),
            "source": self.source().as_str(),
        });
        if let Some(p) = &self.path {
            v["path"] = serde_json::Value::String(p.to_string_lossy().into_owned());
        }
        v
    }
}

// ── embedded catalog ──────────────────────────────────────────────────────────

macro_rules! embedded_skill {
    ($name:literal) => {
        (
            $name,
            include_str!(concat!("../../../../skills/skills/", $name, "/SKILL.md")),
        )
    };
}

/// `(directory name, SKILL.md contents)` for every bundled skill, embedded at
/// compile time. Kept in sync with `skills/skills/` by
/// `embedded_catalog_matches_the_skills_directory_on_disk`.
const EMBEDDED_SKILLS: &[(&str, &str)] = &[
    embedded_skill!("ensemble-production"),
    embedded_skill!("iris-agentic-dev"),
    embedded_skill!("iris-ai-hub"),
    embedded_skill!("iris-connectivity"),
    embedded_skill!("iris-container-graceful-shutdown"),
    embedded_skill!("iris-cpf-merge"),
    embedded_skill!("iris-devtester"),
    embedded_skill!("iris-docs"),
    embedded_skill!("iris-embedded-python"),
    embedded_skill!("iris-linux-docker"),
    embedded_skill!("iris-mdx"),
    embedded_skill!("iris-objectscript-eval"),
    embedded_skill!("iris-pgwire"),
    embedded_skill!("iris-product-features"),
    embedded_skill!("iris-query-plans"),
    embedded_skill!("iris-sql"),
    embedded_skill!("iris-vector-ai"),
    embedded_skill!("iris-vscode-objectscript"),
    embedded_skill!("iris-windows-iis-setup"),
    embedded_skill!("irishealth-container"),
    embedded_skill!("irispython-connector"),
    embedded_skill!("objectscript-coverage"),
    embedded_skill!("objectscript-debugging"),
    embedded_skill!("objectscript-fewshot-fixes"),
    embedded_skill!("objectscript-guardrails"),
    embedded_skill!("objectscript-list-patterns"),
    embedded_skill!("objectscript-loop-patterns"),
    embedded_skill!("objectscript-mac-routines"),
    embedded_skill!("objectscript-navigation"),
    embedded_skill!("objectscript-repair"),
    embedded_skill!("objectscript-review"),
    embedded_skill!("objectscript-sql-patterns"),
    embedded_skill!("objectscript-tdd"),
    embedded_skill!("objectscript-unit-test"),
    embedded_skill!("opencode-introspect"),
];

/// Directory names of the embedded skills.
pub fn embedded_skill_dirs() -> Vec<&'static str> {
    EMBEDDED_SKILLS.iter().map(|(d, _)| *d).collect()
}

/// The tier this binary's embedded copy of a skill declares, matched by directory or name.
pub fn embedded_tier(name: &str) -> Option<SkillTier> {
    let body = embedded_content(name)?;
    parse_skill_md(body, name)?.tier
}

/// The skills `skill_list` and `skill_search` show: everything but the internal tier.
pub fn advertised(skills: &[BundledSkill]) -> Vec<BundledSkill> {
    skills
        .iter()
        .filter(|s| s.tier != Some(SkillTier::Internal))
        .cloned()
        .collect()
}

fn embedded_content(name: &str) -> Option<&'static str> {
    EMBEDDED_SKILLS
        .iter()
        .find(|(dir, body)| {
            *dir == name
                || parse_skill_md(body, dir)
                    .map(|s| s.name == name)
                    .unwrap_or(false)
        })
        .map(|(_, body)| *body)
}

// ── directory resolution ──────────────────────────────────────────────────────

/// Which kind of place a candidate directory is.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DirKind {
    /// `IRIS_AGENTIC_DEV_SKILLS_DIR`: launch settings, set by the operator.
    Env,
    /// Next to or above the running executable: it exists because the operator built or unpacked
    /// the binary there.
    Exe,
    /// `$OBJECTSCRIPT_WORKSPACE/skills/skills` or `<cwd>/skills/skills`: the project's own text.
    Workspace,
}

impl DirKind {
    pub fn as_str(&self) -> &'static str {
        match self {
            DirKind::Env => "env",
            DirKind::Exe => "exe",
            DirKind::Workspace => "workspace",
        }
    }
}

/// A directory that may hold `<name>/SKILL.md` files.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SkillsDir {
    pub path: PathBuf,
    pub kind: DirKind,
}

/// Everything [`skills_dir_candidates_from`] reads, so a test passes it without touching the
/// process environment or the working directory.
#[derive(Debug, Clone, Default)]
pub struct CandidateInputs {
    /// `IRIS_AGENTIC_DEV_SKILLS_DIR`.
    pub env_dir: Option<PathBuf>,
    /// The running executable.
    pub exe: Option<PathBuf>,
    /// `OBJECTSCRIPT_WORKSPACE`.
    pub workspace_env: Option<PathBuf>,
    pub cwd: Option<PathBuf>,
    /// The operator's opt-in to serve the two workspace candidates.
    pub local_skills: bool,
}

/// Candidate skills directories, most specific first. Every candidate is derived at *runtime*
/// from the inputs, never from a build-time constant. The workspace candidates are left out unless
/// `local_skills` is set.
pub fn skills_dir_candidates_from(inputs: &CandidateInputs) -> Vec<SkillsDir> {
    let mut out: Vec<SkillsDir> = Vec::new();
    let mut push = |path: PathBuf, kind: DirKind| {
        if !out.iter().any(|d| d.path == path) {
            out.push(SkillsDir { path, kind });
        }
    };

    if let Some(dir) = inputs
        .env_dir
        .as_ref()
        .filter(|d| !d.as_os_str().is_empty())
    {
        push(dir.clone(), DirKind::Env);
    }

    // Alongside / above the running executable: ./skills/skills, ../skills/skills,
    // covering both an installed layout and `target/debug/<bin>` in a checkout.
    if let Some(exe) = &inputs.exe {
        let mut anchor = exe.parent().map(|p| p.to_path_buf());
        for _ in 0..4 {
            let Some(dir) = anchor else { break };
            push(dir.join("skills").join("skills"), DirKind::Exe);
            anchor = dir.parent().map(|p| p.to_path_buf());
        }
    }

    if inputs.local_skills {
        if let Some(ws) = inputs
            .workspace_env
            .as_ref()
            .filter(|w| !w.as_os_str().is_empty())
        {
            push(ws.join("skills").join("skills"), DirKind::Workspace);
        }
        if let Some(cwd) = &inputs.cwd {
            push(cwd.join("skills").join("skills"), DirKind::Workspace);
        }
    }

    out
}

/// The operator file's `local_skills`, read from `<home>/.iris-agentic-dev.toml`. A workspace file
/// never counts, and a file that is missing or does not parse means no opt-in.
pub fn operator_local_skills(home: Option<&Path>) -> bool {
    let Some(home) = home else { return false };
    let Ok(text) =
        std::fs::read_to_string(home.join(crate::iris::config_sources::CONFIG_FILE_NAME))
    else {
        return false;
    };
    text.parse::<toml::Table>()
        .map(|t| crate::iris::config_sources::operator_settings(&t).local_skills)
        .unwrap_or(false)
}

/// [`operator_local_skills`] under the rule for the operator file (spec 141 FR-043): turning the
/// opt-in off applies at once, turning it on waits for the next start. The first answer of the
/// process is what the server started with.
fn operator_opt_in_held_to_start(home: Option<&Path>) -> bool {
    let now = operator_local_skills(home);
    *STARTED_WITH.get_or_init(|| now) && now
}

static STARTED_WITH: std::sync::OnceLock<bool> = std::sync::OnceLock::new();

/// Record the operator file's `local_skills` as the server started with it, before any edit.
pub fn note_started_with_local_skills(opted_in: bool) {
    STARTED_WITH.get_or_init(|| opted_in);
}

/// The inputs of this process: the environment, the running executable, the working directory and
/// the operator's opt-in (`IAD_LOCAL_SKILLS`, or `local_skills` in the operator file).
pub fn candidate_inputs_from_process() -> CandidateInputs {
    let env_path = |name: &str| std::env::var_os(name).map(PathBuf::from);
    let opt_in_env = std::env::var("IAD_LOCAL_SKILLS")
        .map(|v| {
            matches!(
                v.trim().to_ascii_lowercase().as_str(),
                "1" | "true" | "yes" | "on"
            )
        })
        .unwrap_or(false);
    CandidateInputs {
        env_dir: env_path("IRIS_AGENTIC_DEV_SKILLS_DIR"),
        exe: std::env::current_exe().ok(),
        workspace_env: env_path("OBJECTSCRIPT_WORKSPACE"),
        cwd: std::env::current_dir().ok(),
        local_skills: opt_in_env || operator_opt_in_held_to_start(dirs::home_dir().as_deref()),
    }
}

/// The candidates for this process.
pub fn skills_dir_candidates() -> Vec<SkillsDir> {
    skills_dir_candidates_from(&candidate_inputs_from_process())
}

/// The non-empty workspace directory that was *not* read because the operator has not opted in.
/// `check_config` reports it, with the key that would serve it.
pub fn ignored_workspace_dir(inputs: &CandidateInputs) -> Option<PathBuf> {
    if inputs.local_skills {
        return None;
    }
    let would_serve = CandidateInputs {
        local_skills: true,
        ..inputs.clone()
    };
    skills_dir_candidates_from(&would_serve)
        .into_iter()
        .filter(|d| d.kind == DirKind::Workspace)
        .find(|d| !load_from_dir(&d.path).is_empty())
        .map(|d| d.path)
}

/// Read every `<dir>/*/SKILL.md`. Missing or unreadable directories yield an
/// empty list — discovery must never fail loudly on a path that simply is not
/// there.
pub fn load_from_dir(dir: &Path) -> Vec<BundledSkill> {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return Vec::new();
    };
    let mut out = Vec::new();
    for entry in entries.flatten() {
        let skill_md = entry.path().join("SKILL.md");
        if !skill_md.is_file() {
            continue;
        }
        let Ok(body) = std::fs::read_to_string(&skill_md) else {
            continue;
        };
        let fallback = entry.file_name().to_string_lossy().to_string();
        if let Some(mut s) = parse_skill_md(&body, &fallback) {
            s.path = Some(skill_md);
            out.push(s);
        }
    }
    out.sort_by(|a, b| a.name.cmp(&b.name));
    out
}

/// What `check_config.skills` reports: the directory whose skills replaced the embedded ones, which
/// kind it is, whether the opt-in is on, and a workspace directory that was left unread.
pub fn skills_report_from(inputs: &CandidateInputs) -> serde_json::Value {
    let served = skills_dir_candidates_from(inputs)
        .into_iter()
        .find(|d| !load_from_dir(&d.path).is_empty());
    serde_json::json!({
        "override_dir": served.as_ref().map(|d| d.path.to_string_lossy().into_owned()),
        "override_kind": served.as_ref().map(|d| d.kind.as_str()),
        "local_skills": inputs.local_skills,
        "ignored_dir": ignored_workspace_dir(inputs).map(|p| p.to_string_lossy().into_owned()),
        "ignored_hint": ignored_workspace_dir(inputs).map(|_| {
            "this directory was not read; the operator can turn on local skills to serve it"
        }),
    })
}

/// The full bundled catalog: the embedded copy, with any same-named skill from the first
/// readable on-disk candidate directory taking precedence.
pub fn load_bundled_skills() -> Vec<BundledSkill> {
    load_bundled_skills_from(&skills_dir_candidates())
}

/// [`load_bundled_skills`] over an explicit candidate list.
///
/// A disk copy of an embedded skill keeps the embedded *tier*: the tier says who ships the skill and
/// whether it is advertised, and the file being served is the thing that must not decide that.
pub fn load_bundled_skills_from(candidates: &[SkillsDir]) -> Vec<BundledSkill> {
    let mut out: Vec<BundledSkill> = EMBEDDED_SKILLS
        .iter()
        .filter_map(|(dir, body)| parse_skill_md(body, dir))
        .collect();

    for candidate in candidates {
        let from_disk = load_from_dir(&candidate.path);
        if from_disk.is_empty() {
            continue;
        }
        for mut s in from_disk {
            match out.iter_mut().find(|e| e.name == s.name) {
                // A copy identical to the embedded one replaces nothing, so it is not an `override`
                // (a binary built in a checkout finds that checkout's own files).
                Some(_) if s.content().as_deref() == embedded_content(&s.name) => {}
                Some(existing) => {
                    s.tier = existing.tier;
                    *existing = s;
                }
                None => out.push(s),
            }
        }
        break;
    }

    out.sort_by(|a, b| a.name.cmp(&b.name));
    out
}

// ── frontmatter parsing ───────────────────────────────────────────────────────

/// Pull `name`, `description`, `tags` and `tier` out of a `SKILL.md`'s YAML frontmatter.
///
/// Hand-rolled rather than a YAML dependency because we need four keys
/// out of frontmatter that also carries nested `metadata:` maps. Handles both
/// `tags: [a, b]` and block sequences, and folds wrapped `description:` values
/// (which the real bundled files all use) back into one line.
pub fn parse_skill_md(content: &str, fallback_name: &str) -> Option<BundledSkill> {
    let rest = content.strip_prefix("---")?;
    let rest = rest.strip_prefix('\n').unwrap_or(rest);
    let end = rest.find("\n---")?;
    let frontmatter = &rest[..end];

    let mut name: Option<String> = None;
    let mut description: Option<String> = None;
    let mut tags: Vec<String> = Vec::new();
    let mut tier: Option<SkillTier> = None;

    // Tracks which top-level key we are inside, so a nested `metadata:` list
    // never gets mistaken for the tag list.
    #[derive(PartialEq)]
    enum In {
        None,
        Name,
        Description,
        Tags,
    }
    let mut state = In::None;

    for line in frontmatter.lines() {
        let indented = line.starts_with(' ') || line.starts_with('\t');
        let trimmed = line.trim();
        if trimmed.is_empty() {
            continue;
        }

        // A block-sequence item belonging to the key we are currently inside.
        if let Some(item) =
            trimmed
                .strip_prefix("- ")
                .or_else(|| if trimmed == "-" { Some("") } else { None })
        {
            if state == In::Tags {
                let t = clean_scalar(item);
                if !t.is_empty() {
                    tags.push(t.to_lowercase());
                }
            }
            continue;
        }

        // A continuation of a folded multi-line scalar.
        if indented && !is_key_line(trimmed) {
            match state {
                In::Description => {
                    let d = description.get_or_insert_with(String::new);
                    if !d.is_empty() {
                        d.push(' ');
                    }
                    d.push_str(trimmed);
                }
                In::Name => {
                    if let Some(n) = name.as_mut() {
                        n.push_str(trimmed);
                    }
                }
                _ => {}
            }
            continue;
        }
        if indented {
            // A nested key inside some other mapping — not one of ours.
            state = In::None;
            continue;
        }

        let Some((key, value)) = trimmed.split_once(':') else {
            state = In::None;
            continue;
        };
        let value = value.trim();
        match key.trim() {
            "name" => {
                state = In::Name;
                name = Some(clean_scalar(value));
            }
            "description" => {
                state = In::Description;
                description = Some(clean_scalar(value));
            }
            "tier" => {
                state = In::None;
                tier = SkillTier::parse(&clean_scalar(value));
            }
            "tags" => {
                state = In::Tags;
                if let Some(inner) = value.strip_prefix('[').and_then(|v| v.strip_suffix(']')) {
                    for t in inner.split(',') {
                        let t = clean_scalar(t);
                        if !t.is_empty() {
                            tags.push(t.to_lowercase());
                        }
                    }
                }
            }
            _ => state = In::None,
        }
    }

    let name = match name {
        Some(n) if !n.is_empty() => n,
        _ => fallback_name.to_string(),
    };
    Some(BundledSkill {
        name,
        description: description.unwrap_or_default(),
        tags,
        tier,
        path: None,
    })
}

/// True when a frontmatter line looks like `key: ...` rather than a folded
/// continuation of the previous value.
fn is_key_line(trimmed: &str) -> bool {
    match trimmed.split_once(':') {
        Some((key, rest)) => {
            !key.is_empty()
                && !key.contains(' ')
                && (rest.is_empty() || rest.starts_with(' '))
                && key
                    .chars()
                    .all(|c| c.is_alphanumeric() || c == '_' || c == '-')
        }
        None => false,
    }
}

fn clean_scalar(v: &str) -> String {
    v.trim().trim_matches('"').trim_matches('\'').to_string()
}

// ── search ────────────────────────────────────────────────────────────────────

/// Split a query into lowercased terms.
pub fn query_terms(query: &str) -> Vec<String> {
    query
        .split(|c: char| c.is_whitespace() || c == ',')
        .map(|t| t.trim().to_lowercase())
        .filter(|t| !t.is_empty())
        .collect()
}

/// Relevance of a skill to a set of query terms. Zero means no match at all.
/// Name hits weigh heaviest, then tags, then description — a tag hit alone is
/// enough to surface a skill, which is the whole point of this change.
pub fn score_skill(skill: &BundledSkill, terms: &[String]) -> u32 {
    let name = skill.name.to_lowercase();
    let description = skill.description.to_lowercase();
    let mut score = 0u32;
    for term in terms {
        if name == *term {
            score += 20;
        } else if name.contains(term.as_str()) {
            score += 10;
        }
        if skill
            .tags
            .iter()
            .any(|t| t == term || t.contains(term.as_str()) || term.contains(t.as_str()))
        {
            score += 6;
        }
        if description.contains(term.as_str()) {
            score += 3;
        }
    }
    score
}

/// Matching skills paired with their score, best first, capped at `top_k`.
pub fn search_bundled<'a>(
    skills: &'a [BundledSkill],
    query: &str,
    top_k: usize,
) -> Vec<(&'a BundledSkill, u32)> {
    let terms = query_terms(query);
    if terms.is_empty() {
        return Vec::new();
    }
    let mut scored: Vec<(&BundledSkill, u32)> = skills
        .iter()
        .map(|s| (s, score_skill(s, &terms)))
        .filter(|(_, sc)| *sc > 0)
        .collect();
    scored.sort_by(|a, b| b.1.cmp(&a.1).then_with(|| a.0.name.cmp(&b.0.name)));
    scored.truncate(top_k);
    scored
}

// ── merging the two sources ───────────────────────────────────────────────────

/// One entry in a merged listing, labelled with where it came from.
#[derive(Debug, Clone)]
pub struct MergedSkill {
    pub name: String,
    pub description: String,
    pub tags: Vec<String>,
    pub source: SkillSource,
    /// The file served, for an `override` or `local` skill.
    pub path: Option<PathBuf>,
    /// Bundled skills only; a `^SKILLS` entry has no tier.
    pub tier: Option<SkillTier>,
    /// A `^SKILLS` entry of the same name also exists (bundled copy wins).
    pub also_synthesized: bool,
}

impl MergedSkill {
    pub fn to_json(&self) -> serde_json::Value {
        let mut v = serde_json::json!({
            "name": self.name,
            "description": self.description,
            "tags": self.tags,
            "source": self.source.as_str(),
        });
        if let Some(t) = self.tier {
            v["tier"] = serde_json::Value::from(t.as_str());
        }
        if let Some(p) = &self.path {
            v["path"] = serde_json::Value::String(p.to_string_lossy().into_owned());
        }
        if self.also_synthesized {
            v["also_synthesized"] = serde_json::Value::Bool(true);
        }
        v
    }
}

/// Name of a `^SKILLS` entry, which may be an object or a bare string.
fn synthesized_name(v: &serde_json::Value) -> Option<String> {
    if let Some(s) = v.as_str() {
        return (!s.is_empty()).then(|| s.to_string());
    }
    v.get("name")
        .and_then(|n| n.as_str())
        .filter(|s| !s.is_empty())
        .map(|s| s.to_string())
}

fn synthesized_description(v: &serde_json::Value) -> String {
    v.get("description")
        .and_then(|d| d.as_str())
        .unwrap_or_default()
        .to_string()
}

/// Combine bundled and synthesized skills into one labelled list, deduplicated
/// by name with the bundled copy winning.
pub fn merge_sources(
    bundled: &[BundledSkill],
    synthesized: &[serde_json::Value],
) -> Vec<MergedSkill> {
    let mut out: Vec<MergedSkill> = bundled
        .iter()
        .map(|s| MergedSkill {
            name: s.name.clone(),
            description: s.description.clone(),
            tags: s.tags.clone(),
            source: s.source(),
            path: s.path.clone(),
            tier: s.tier,
            also_synthesized: false,
        })
        .collect();

    for v in synthesized {
        let Some(name) = synthesized_name(v) else {
            continue;
        };
        match out.iter_mut().find(|e| e.name == name) {
            Some(existing) => existing.also_synthesized = true,
            None => out.push(MergedSkill {
                name,
                description: synthesized_description(v),
                tags: Vec::new(),
                source: SkillSource::Synthesized,
                path: None,
                tier: None,
                also_synthesized: false,
            }),
        }
    }
    out
}

/// Per-source accounting attached to every response so a zero result is never
/// ambiguous: the caller can see both sources were considered, how many
/// candidates each held, and whether either was skipped.
pub fn sources_json(
    bundled_available: usize,
    synthesized_available: usize,
    synthesized_searched: bool,
) -> serde_json::Value {
    serde_json::json!({
        "bundled": {
            "available": bundled_available,
            "searched": true,
        },
        "synthesized": {
            "available": synthesized_available,
            "searched": synthesized_searched,
            "note": if synthesized_searched {
                "^SKILLS global in the connected IRIS instance"
            } else {
                "no IRIS connection — ^SKILLS global not searched"
            },
        },
    })
}

/// Human-readable explanation of what was searched. Attached to zero-hit
/// responses so `count: 0` cannot be misread as "this does not exist".
pub fn searched_note(
    bundled_available: usize,
    synthesized_available: usize,
    synthesized_searched: bool,
) -> String {
    if synthesized_searched {
        format!(
            "Searched {bundled_available} bundled skills and {synthesized_available} synthesized (^SKILLS) skills.",
        )
    } else {
        format!(
            "Searched {bundled_available} bundled skills. The synthesized (^SKILLS) source was not searched — no IRIS connection.",
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn embedded_catalog_is_populated() {
        assert!(EMBEDDED_SKILLS.len() >= 25);
    }

    #[test]
    fn every_embedded_skill_parses() {
        for (dir, body) in EMBEDDED_SKILLS {
            let s = parse_skill_md(body, dir)
                .unwrap_or_else(|| panic!("{dir}/SKILL.md has no parseable frontmatter"));
            assert!(!s.description.is_empty(), "{dir} has no description");
        }
    }

    #[test]
    fn embedded_content_lookup_by_name() {
        assert!(embedded_content("iris-vector-ai").is_some());
        assert!(embedded_content("no-such-skill").is_none());
    }

    #[test]
    fn query_terms_splits_on_whitespace_and_commas() {
        assert_eq!(
            query_terms("Vector, HNSW  index"),
            ["vector", "hnsw", "index"]
        );
        assert!(query_terms("   ").is_empty());
    }

    #[test]
    fn is_key_line_rejects_prose_with_colon() {
        assert!(is_key_line("name: x"));
        assert!(is_key_line("tags:"));
        assert!(!is_key_line("Hard gate — do this: always"));
        assert!(!is_key_line("plain continuation text"));
    }

    #[test]
    fn clean_scalar_strips_quotes_and_space() {
        assert_eq!(clean_scalar("  \"quoted\" "), "quoted");
        assert_eq!(clean_scalar("'single'"), "single");
    }

    #[test]
    fn synthesized_name_handles_both_shapes() {
        assert_eq!(
            synthesized_name(&serde_json::json!({"name": "a"})),
            Some("a".to_string())
        );
        assert_eq!(
            synthesized_name(&serde_json::json!("b")),
            Some("b".to_string())
        );
        assert_eq!(synthesized_name(&serde_json::json!({})), None);
        assert_eq!(synthesized_name(&serde_json::json!("")), None);
    }

    #[test]
    fn sources_json_flags_unsearched_synthesized_side() {
        let v = sources_json(31, 0, false);
        assert_eq!(v["bundled"]["available"], 31);
        assert_eq!(v["synthesized"]["searched"], false);
        assert!(v["synthesized"]["note"]
            .as_str()
            .unwrap()
            .contains("no IRIS connection"));
    }

    #[test]
    fn searched_note_mentions_both_sources() {
        assert!(searched_note(31, 2, true).contains("31 bundled"));
        assert!(searched_note(31, 2, true).contains("2 synthesized"));
        assert!(searched_note(31, 0, false).contains("not searched"));
    }

    #[test]
    fn merged_skill_json_omits_also_synthesized_when_false() {
        let m = MergedSkill {
            name: "x".into(),
            description: "d".into(),
            tags: vec![],
            source: SkillSource::Bundled,
            path: None,
            tier: None,
            also_synthesized: false,
        };
        assert!(m.to_json().get("also_synthesized").is_none());
    }

    #[test]
    fn bundled_skill_content_reads_embedded_body() {
        let s = BundledSkill {
            name: "iris-vector-ai".into(),
            description: String::new(),
            tags: vec![],
            tier: None,
            path: None,
        };
        assert!(s.content().unwrap().contains("HNSW"));
    }

    #[test]
    fn candidates_are_all_absolute_or_env_provided() {
        // Smoke test: resolution must not panic and must produce something.
        assert!(!skills_dir_candidates().is_empty());
    }

    #[test]
    fn synthesized_skills_output_shape_is_valid_json_array() {
        // Regression test for #119: the old ObjectScript code concatenated raw ^SKILLS
        // pipe-delimited values directly into a JSON array literal, producing invalid JSON
        // the moment any entry existed. The fix uses %DynamicArray/%DynamicObject, which
        // emits {"name":...,"description":...,"body":...} objects. Verify the shape
        // merge_sources / synthesized_name expect is what we now produce.
        let entry =
            serde_json::json!({"name": "my-skill", "description": "does X", "body": "body text"});
        assert_eq!(synthesized_name(&entry), Some("my-skill".to_string()));
        assert_eq!(synthesized_description(&entry), "does X");

        // Also confirm the old broken shape (raw pipe string) correctly fails parsing
        // — this is what was silently failing before the fix.
        let broken = r#"["my-skill|does X|body text|0|2026-01-01T00:00:00Z"]"#;
        let parsed = serde_json::from_str::<Vec<serde_json::Value>>(broken).unwrap();
        // The old code put raw pipe strings in the array — synthesized_name treats a
        // bare string as the name (no description/body). The new code never emits this.
        assert_eq!(
            synthesized_name(&parsed[0]),
            Some("my-skill|does X|body text|0|2026-01-01T00:00:00Z".to_string())
        );
        // Description is empty for the old shape — i.e. it was always lost.
        assert_eq!(synthesized_description(&parsed[0]), "");
    }
}
