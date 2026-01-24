use gix::Repository;
use memmap2::{MmapMut, MmapOptions};
use serde::Deserialize;
use std::borrow::Cow;
use std::env;
use std::fs::{self, OpenOptions};
use std::io::{self, BufWriter, Read, Write};
use std::path::Path;
use std::process::{Command, Stdio};
use std::sync::OnceLock;
use std::time::SystemTime;

static HOME_DIR: OnceLock<String> = OnceLock::new();

fn get_home() -> &'static str {
    HOME_DIR.get_or_init(|| env::var("HOME").unwrap_or_default())
}

// Tokyo Night Colors (bright)
const RESET: &str = "\x1b[0m";
const TN_BLUE: &str = "\x1b[38;2;122;162;247m";
const TN_CYAN: &str = "\x1b[38;2;125;207;255m";
const TN_PURPLE: &str = "\x1b[38;2;187;154;247m";
const TN_MAGENTA: &str = "\x1b[38;2;157;124;216m";
const TN_GREEN: &str = "\x1b[38;2;158;206;106m";
const TN_ORANGE: &str = "\x1b[38;2;255;158;100m";
const TN_TEAL: &str = "\x1b[38;2;42;195;222m";
const TN_GRAY: &str = "\x1b[38;2;120;140;180m";
const TN_RED: &str = "\x1b[38;2;247;118;142m";

const SEP: &str = "\x1b[38;2;86;95;137m • \x1b[0m";

// OSC 8 hyperlink escape sequences (using BEL terminator for broader compatibility)
const OSC8_START: &str = "\x1b]8;;";
const OSC8_MID: &str = "\x07";
const OSC8_END: &str = "\x1b]8;;\x07";

const TERM_WIDTH: usize = 50;

fn hash_path(path: &str) -> u64 {
    path.bytes().fold(0u64, |acc, b| acc.wrapping_mul(31).wrapping_add(b as u64))
}

#[derive(Deserialize, Default)]
struct ClaudeInput {
    #[serde(default)]
    model: Model,
    #[serde(default)]
    context_window: ContextWindow,
    #[serde(default)]
    cost: Cost,
    #[serde(default)]
    output_style: OutputStyle,
    #[serde(default)]
    workspace: Workspace,
}

#[derive(Deserialize, Default)]
struct Model {
    #[serde(default)]
    display_name: Option<String>,
}

#[derive(Deserialize, Default)]
struct ContextWindow {
    #[serde(default)]
    remaining_percentage: Option<f64>,
    #[serde(default)]
    total_input_tokens: Option<u64>,
    #[serde(default)]
    total_output_tokens: Option<u64>,
}

#[derive(Deserialize, Default)]
struct Cost {
    #[serde(default)]
    total_duration_ms: Option<u64>,
}

#[derive(Deserialize, Default)]
struct OutputStyle {
    #[serde(default)]
    name: Option<String>,
}

#[derive(Deserialize, Default)]
struct Workspace {
    #[serde(default)]
    project_dir: Option<String>,
    #[serde(default)]
    current_dir: Option<String>,
}

/// Binary cache format for mmap (fixed 128 bytes)
const CACHE_SIZE: usize = 128;
const CACHE_MAGIC: &[u8; 4] = b"CCST";
const CACHE_VERSION: u32 = 1;

struct MmapCache {
    index_mtime: u64,
    head_oid: [u8; 40],
    files_changed: u32,
    lines_added: u32,
    lines_deleted: u32,
    ahead: u32,
    behind: u32,
}

impl Default for MmapCache {
    fn default() -> Self {
        Self {
            index_mtime: 0,
            head_oid: [0u8; 40],
            files_changed: 0,
            lines_added: 0,
            lines_deleted: 0,
            ahead: 0,
            behind: 0,
        }
    }
}

impl MmapCache {
    fn from_bytes(data: &[u8]) -> Option<Self> {
        if data.len() < CACHE_SIZE || &data[0..4] != CACHE_MAGIC {
            return None;
        }
        let version = u32::from_le_bytes(data[4..8].try_into().ok()?);
        if version != CACHE_VERSION {
            return None;
        }

        let mut cache = MmapCache::default();
        cache.index_mtime = u64::from_le_bytes(data[8..16].try_into().ok()?);
        cache.head_oid.copy_from_slice(&data[16..56]);
        cache.files_changed = u32::from_le_bytes(data[56..60].try_into().ok()?);
        cache.lines_added = u32::from_le_bytes(data[60..64].try_into().ok()?);
        cache.lines_deleted = u32::from_le_bytes(data[64..68].try_into().ok()?);
        cache.ahead = u32::from_le_bytes(data[68..72].try_into().ok()?);
        cache.behind = u32::from_le_bytes(data[72..76].try_into().ok()?);
        Some(cache)
    }

    fn to_bytes(&self, buf: &mut [u8]) {
        buf[0..4].copy_from_slice(CACHE_MAGIC);
        buf[4..8].copy_from_slice(&CACHE_VERSION.to_le_bytes());
        buf[8..16].copy_from_slice(&self.index_mtime.to_le_bytes());
        buf[16..56].copy_from_slice(&self.head_oid);
        buf[56..60].copy_from_slice(&self.files_changed.to_le_bytes());
        buf[60..64].copy_from_slice(&self.lines_added.to_le_bytes());
        buf[64..68].copy_from_slice(&self.lines_deleted.to_le_bytes());
        buf[68..72].copy_from_slice(&self.ahead.to_le_bytes());
        buf[72..76].copy_from_slice(&self.behind.to_le_bytes());
    }

    fn head_oid_matches(&self, oid: &str) -> bool {
        let oid_bytes = oid.as_bytes();
        oid_bytes.len() <= 40 && self.head_oid[..oid_bytes.len()] == *oid_bytes
    }
}

// ============================================================================
// PR Cache
// ============================================================================

/// PR cache data - parsed from gh JSON output
#[derive(Default, Clone)]
struct PrCacheData {
    number: u32,
    state: String,
    url: String,
    comments: u32,
    changed_files: u32,
    check_status: String, // "passed", "failed", "pending", ""
}

/// JSON structure from gh pr view
#[derive(Deserialize, Default)]
struct GhPrJson {
    number: Option<u64>,
    state: Option<String>,
    url: Option<String>,
    comments: Option<Vec<serde_json::Value>>,
    #[serde(rename = "changedFiles")]
    changed_files: Option<u64>,
    #[serde(rename = "statusCheckRollup")]
    status_check_rollup: Option<Vec<GhCheckRun>>,
}

#[derive(Deserialize)]
struct GhCheckRun {
    conclusion: Option<String>,
}

const PR_CACHE_TTL: u64 = 60; // seconds

fn get_pr_cache_path(repo_path: &str, branch: &str) -> String {
    let key = format!("{}:{}", repo_path, branch);
    format!("/tmp/cc-pr-{:016x}.cache", hash_path(&key))
}

fn load_pr_cache(repo_path: &str, branch: &str) -> Option<PrCacheData> {
    let cache_path = get_pr_cache_path(repo_path, branch);
    let content = fs::read_to_string(&cache_path).ok()?;

    // First line is timestamp, rest is JSON
    let mut lines = content.lines();
    let timestamp: u64 = lines.next()?.parse().ok()?;
    let cached_branch = lines.next()?;

    // Validate branch matches
    if cached_branch != branch {
        let _ = fs::remove_file(&cache_path);
        return None;
    }

    // Check TTL
    let now = SystemTime::now()
        .duration_since(SystemTime::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0);
    if now.saturating_sub(timestamp) > PR_CACHE_TTL {
        return None;
    }

    // Rest is JSON
    let json_str: String = lines.collect::<Vec<_>>().join("\n");
    let pr: GhPrJson = serde_json::from_str(&json_str).ok()?;

    // Compute check status from rollup
    let check_status = match &pr.status_check_rollup {
        None => String::new(),
        Some(checks) if checks.is_empty() => String::new(),
        Some(checks) => {
            // Treat any non-success conclusion as a failure
            let has_failure = checks.iter().any(|c| {
                match c.conclusion.as_deref() {
                    Some("SUCCESS") | Some("SKIPPED") | Some("NEUTRAL") => false,
                    Some(_) => true, // FAILURE, CANCELLED, TIMED_OUT, ACTION_REQUIRED, etc.
                    None => false,
                }
            });
            let has_pending = checks.iter().any(|c| c.conclusion.is_none());
            let all_passed = checks.iter().all(|c| {
                matches!(
                    c.conclusion.as_deref(),
                    Some("SUCCESS") | Some("SKIPPED") | Some("NEUTRAL")
                )
            });

            if has_failure {
                "failed".to_string()
            } else if all_passed {
                "passed".to_string()
            } else if has_pending {
                "pending".to_string()
            } else {
                String::new()
            }
        }
    };

    Some(PrCacheData {
        number: pr.number.unwrap_or(0) as u32,
        state: pr.state.unwrap_or_default(),
        url: pr.url.unwrap_or_default(),
        comments: pr.comments.map(|c| c.len() as u32).unwrap_or(0),
        changed_files: pr.changed_files.unwrap_or(0) as u32,
        check_status,
    })
}

// ============================================================================
// PR Fetch (background only)
// ============================================================================

/// Check if remote is GitHub
/// Resolves the common git directory for worktree support
fn is_github_remote(git_dir: &str) -> bool {
    // In linked worktrees, git_dir is .git/worktrees/<name>, not the main .git dir.
    // The config with remotes lives in the common dir, so resolve it first.
    let common_dir = Command::new("git")
        .args(["--git-dir", git_dir, "rev-parse", "--git-common-dir"])
        .output()
        .ok()
        .filter(|o| o.status.success())
        .map(|o| String::from_utf8_lossy(&o.stdout).trim().to_string())
        .filter(|s| !s.is_empty())
        .unwrap_or_else(|| git_dir.trim_end_matches('/').to_string());

    let config_path = Path::new(&common_dir).join("config");
    if let Ok(content) = fs::read_to_string(&config_path) {
        return content.contains("github.com");
    }
    false
}

/// Spawn background process to refresh PR cache
fn spawn_pr_refresh(git_dir: &str, branch: &str) {
    // Only proceed if this is a GitHub repo
    if !is_github_remote(git_dir) {
        return;
    }

    // Get working directory (parent of .git)
    let work_dir = Path::new(git_dir)
        .parent()
        .map(|p| p.to_string_lossy().to_string())
        .unwrap_or_else(|| ".".to_string());

    let cache_path = get_pr_cache_path(git_dir, branch);
    let now = SystemTime::now()
        .duration_since(SystemTime::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0);

    // Minimal shell to detach gh process and write cache with timestamp header
    let cmd = format!(
        r#"cd '{}' && json=$(gh pr view --json number,state,url,comments,changedFiles,statusCheckRollup 2>/dev/null) && [ -n "$json" ] && printf '%s\n%s\n%s' '{}' '{}' "$json" > '{}'"#,
        work_dir, now, branch, cache_path
    );

    let _ = Command::new("sh")
        .args(["-c", &cmd])
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn();
}

/// Get PR data - checks cache first, spawns refresh if needed
/// Returns immediately with cached data or None (never blocks on network)
fn get_pr_data(git: &GitRepo) -> Option<PrCacheData> {
    // Try cache first (fast path)
    if let Some(cache) = load_pr_cache(&git.git_dir, &git.branch) {
        return Some(cache);
    }

    // Cache miss or stale - spawn background refresh only
    // Don't block - return None and let the next render show PR data
    spawn_pr_refresh(&git.git_dir, &git.branch);

    None
}

/// Holds repository state for lazy evaluation of expensive git operations
struct GitRepo {
    repo: Repository,
    branch: String,
    worktree: Option<String>,
    git_dir: String,
}

impl GitRepo {
    /// Compute diff stats using git index - simplified, just count modified files
    fn diff_stats(&self) -> Option<(u32, u32, u32)> {
        let index = self.repo.index().ok()?;
        let workdir = self.repo.work_dir()?;
        let mut files = 0u32;

        for entry in index.entries() {
            let path_bstr = entry.path(&index);
            let path_str = std::str::from_utf8(path_bstr.as_ref()).ok()?;
            let file_path = workdir.join(path_str);

            if let Ok(metadata) = fs::metadata(&file_path) {
                let mtime = metadata.modified().ok()?
                    .duration_since(SystemTime::UNIX_EPOCH).ok()?
                    .as_secs();
                let index_mtime = entry.stat.mtime.secs as u64;

                if mtime != index_mtime {
                    files += 1;
                }
            } else {
                files += 1; // File deleted
            }
        }

        // gix doesn't easily give line counts, so just return file count
        Some((files, 0, 0))
    }

    /// Get index mtime for cache invalidation
    fn index_mtime(&self) -> u64 {
        let index_path = format!("{}/index", self.git_dir.trim_end_matches('/'));
        fs::metadata(&index_path)
            .and_then(|m| m.modified())
            .map(|t| t.duration_since(SystemTime::UNIX_EPOCH).unwrap_or_default().as_secs())
            .unwrap_or(0)
    }

    /// Get HEAD oid for cache invalidation
    fn head_oid(&self) -> String {
        let ref_path = format!("{}/refs/heads/{}", self.git_dir.trim_end_matches('/'), self.branch);
        if let Ok(oid) = fs::read_to_string(&ref_path) {
            return oid.trim().to_string();
        }
        self.repo.head_id()
            .map(|id| id.to_string())
            .unwrap_or_default()
    }
}

fn get_cache_path(git_dir: &str) -> String {
    format!("/tmp/cc-status-{:016x}.cache", hash_path(git_dir))
}

fn load_mmap_cache(git_dir: &str) -> Option<MmapCache> {
    let cache_path = get_cache_path(git_dir);
    let file = OpenOptions::new().read(true).open(&cache_path).ok()?;
    let mmap = unsafe { MmapOptions::new().map(&file).ok()? };
    MmapCache::from_bytes(&mmap)
}

fn save_mmap_cache(git_dir: &str, cache: &MmapCache) {
    let cache_path = get_cache_path(git_dir);
    let file = match OpenOptions::new()
        .read(true).write(true).create(true).truncate(true)
        .open(&cache_path)
    {
        Ok(f) => f,
        Err(_) => return,
    };
    if file.set_len(CACHE_SIZE as u64).is_err() {
        return;
    }
    let mut mmap = match unsafe { MmapMut::map_mut(&file) } {
        Ok(m) => m,
        Err(_) => return,
    };
    cache.to_bytes(&mut mmap);
    let _ = mmap.flush();
}

struct GitPathCache {
    git_path: String,
    branch: String,
}

fn get_head_mtime(git_path: &str) -> u64 {
    let head_path = format!("{}/HEAD", git_path.trim_end_matches('/'));
    fs::metadata(&head_path)
        .and_then(|m| m.modified())
        .map(|t| t.duration_since(SystemTime::UNIX_EPOCH).unwrap_or_default().as_secs())
        .unwrap_or(0)
}

fn get_cached_git_info(working_dir: &str) -> Option<GitPathCache> {
    let cache_path = format!("/tmp/cc-gitpath-{:016x}.cache", hash_path(working_dir));
    let content = fs::read_to_string(&cache_path).ok()?;
    let mut lines = content.lines();

    let git_path = lines.next()?.to_string();
    let branch = lines.next()?.to_string();
    let cached_mtime: u64 = lines.next()?.parse().ok()?;

    if !Path::new(&git_path).exists() {
        let _ = fs::remove_file(&cache_path);
        return None;
    }

    let current_mtime = get_head_mtime(&git_path);
    if current_mtime != cached_mtime {
        return None;
    }

    Some(GitPathCache { git_path, branch })
}

fn cache_git_info(working_dir: &str, git_path: &str, branch: &str) {
    let cache_path = format!("/tmp/cc-gitpath-{:016x}.cache", hash_path(working_dir));
    let head_mtime = get_head_mtime(git_path);
    let content = format!("{}\n{}\n{}", git_path, branch, head_mtime);
    let _ = fs::write(&cache_path, content);
}

fn main() {
    let mut input = String::with_capacity(4096);
    io::stdin().read_to_string(&mut input).unwrap_or_default();

    let data: ClaudeInput = serde_json::from_str(&input).unwrap_or_default();

    let current_dir: Cow<str> = match data.workspace.current_dir.as_deref() {
        Some(dir) => Cow::Borrowed(dir),
        None => Cow::Owned(env::current_dir().unwrap().to_string_lossy().into_owned()),
    };

    let stdout = io::stdout();
    let mut out = BufWriter::new(stdout.lock());

    write_row1(&mut out, &data, &current_dir);
    let git_repo = get_git_repo(&current_dir);
    write_row2(&mut out, git_repo.as_ref());
    write_pr_rows(&mut out, git_repo.as_ref());
    write_row3(&mut out, &data);
    write_row4(&mut out, &data);

    out.flush().unwrap_or_default();
}

fn write_row1<W: Write>(out: &mut W, data: &ClaudeInput, current_dir: &str) {
    let project_name = data
        .workspace
        .project_dir
        .as_ref()
        .and_then(|p| Path::new(p).file_name())
        .map(|n| n.to_string_lossy())
        .unwrap_or_default();

    let home = get_home();
    let display_cwd: Cow<str> = if !home.is_empty() && current_dir.starts_with(home) {
        Cow::Owned(format!("~{}", &current_dir[home.len()..]))
    } else {
        Cow::Borrowed(&current_dir)
    };

    let path_width = TERM_WIDTH.saturating_sub(project_name.len()).saturating_sub(3).max(10);
    let abbrev_cwd = abbreviate_path(&display_cwd, path_width);

    writeln!(out, "{TN_BLUE}{project_name}{RESET}{SEP}{TN_CYAN}{abbrev_cwd}{RESET}").unwrap_or_default();
}

fn abbreviate_path(path: &str, max_width: usize) -> Cow<'_, str> {
    if path.len() <= max_width {
        return Cow::Borrowed(path);
    }

    let bytes = path.as_bytes();
    let mut seg_starts: [usize; 32] = [0; 32];
    let mut seg_count = 1;
    seg_starts[0] = 0;

    for (i, &b) in bytes.iter().enumerate() {
        if b == b'/' && seg_count < 32 {
            seg_starts[seg_count] = i + 1;
            seg_count += 1;
        }
    }

    if seg_count < 2 {
        return Cow::Borrowed(path);
    }

    let last_start = seg_starts[seg_count - 1];
    let parent_start = seg_starts[seg_count - 2];
    let last_seg = &path[last_start..];
    let parent_seg = &path[parent_start..last_start.saturating_sub(1)];

    let abbrev_prefix_len = (seg_count - 2) * 2;
    let try1_len = abbrev_prefix_len + parent_seg.len() + 1 + last_seg.len();

    let mut result = String::with_capacity(max_width + 10);

    if try1_len <= max_width || seg_count <= 2 {
        for i in 0..seg_count.saturating_sub(2) {
            let start = seg_starts[i];
            if start < bytes.len() && bytes[start] != b'/' {
                result.push(bytes[start] as char);
                result.push('/');
            }
        }
        result.push_str(parent_seg);
        result.push('/');
        result.push_str(last_seg);
    } else {
        for i in 0..seg_count - 1 {
            let start = seg_starts[i];
            if start < bytes.len() && bytes[start] != b'/' {
                result.push(bytes[start] as char);
                result.push('/');
            }
        }
        result.push_str(last_seg);
    }

    Cow::Owned(result)
}

/// Detect linked worktree name from git_dir path
fn get_worktree_name(git_dir: &str) -> Option<String> {
    // Linked worktrees have git_dir like: /path/.git/worktrees/<name>
    if let Some(idx) = git_dir.find("/.git/worktrees/") {
        let name = &git_dir[idx + 16..]; // skip "/.git/worktrees/"
        let name = name.trim_end_matches('/');
        if !name.is_empty() {
            return Some(name.to_string());
        }
    }
    None
}

fn get_git_repo(dir: &str) -> Option<GitRepo> {
    // Try cache first
    if let Some(cache) = get_cached_git_info(dir) {
        let repo = gix::open(&cache.git_path).ok()?;
        let worktree = get_worktree_name(&cache.git_path);
        return Some(GitRepo {
            repo,
            branch: cache.branch,
            worktree,
            git_dir: cache.git_path,
        });
    }

    // Discover repo
    let repo = gix::discover(dir).ok()?;
    let git_dir = repo.git_dir().to_string_lossy().into_owned();

    // Get branch name from HEAD
    let head = repo.head().ok()?;
    let branch = head.referent_name()
        .map(|n| n.shorten().to_string())
        .unwrap_or_else(|| "HEAD".to_string());

    let worktree = get_worktree_name(&git_dir);

    cache_git_info(dir, &git_dir, &branch);
    Some(GitRepo { repo, branch, worktree, git_dir })
}

fn write_row2<W: Write>(out: &mut W, git: Option<&GitRepo>) {
    let git = match git {
        None => {
            writeln!(out, "{TN_GRAY}no git{RESET}").unwrap_or_default();
            return;
        }
        Some(g) => g,
    };

    write!(out, "{TN_PURPLE}{}{RESET}", git.branch).unwrap_or_default();

    if let Some(wt) = &git.worktree {
        write!(out, "{SEP}{TN_MAGENTA}{wt}{RESET}").unwrap_or_default();
    }

    // Try mmap cache first
    let cache = load_mmap_cache(&git.git_dir);
    let current_mtime = git.index_mtime();
    let current_oid = git.head_oid();

    let (files_changed, lines_added, lines_deleted, ahead, behind) =
        if let Some(ref c) = cache {
            if c.index_mtime == current_mtime && c.head_oid_matches(&current_oid) {
                (c.files_changed, c.lines_added, c.lines_deleted, c.ahead, c.behind)
            } else {
                compute_and_cache_git_stats(git, current_mtime, &current_oid)
            }
        } else {
            compute_and_cache_git_stats(git, current_mtime, &current_oid)
        };

    if files_changed > 0 || lines_added > 0 || lines_deleted > 0 {
        write!(out, "{SEP}").unwrap_or_default();
        if files_changed > 0 {
            write!(out, "{TN_GRAY}{files_changed} files{RESET}").unwrap_or_default();
        }
        if lines_added > 0 {
            if files_changed > 0 { write!(out, " ").unwrap_or_default(); }
            write!(out, "{TN_GREEN}+{lines_added}{RESET}").unwrap_or_default();
        }
        if lines_deleted > 0 {
            if files_changed > 0 || lines_added > 0 { write!(out, " ").unwrap_or_default(); }
            write!(out, "{TN_RED}-{lines_deleted}{RESET}").unwrap_or_default();
        }
    }

    if ahead > 0 || behind > 0 {
        write!(out, "{SEP}").unwrap_or_default();
        if ahead > 0 {
            write!(out, "{TN_GRAY}↑{ahead}{RESET}").unwrap_or_default();
        }
        if behind > 0 {
            if ahead > 0 { write!(out, " ").unwrap_or_default(); }
            write!(out, "{TN_GRAY}↓{behind}{RESET}").unwrap_or_default();
        }
    }

    writeln!(out).unwrap_or_default();
}

/// Write PR info rows (only shown when a PR exists for current branch)
fn write_pr_rows<W: Write>(out: &mut W, git: Option<&GitRepo>) {
    let git = match git {
        None => return,
        Some(g) => g,
    };

    let pr = match get_pr_data(git) {
        None => return,
        Some(p) => p,
    };

    // PR number (cyan, clickable via OSC 8)
    if !pr.url.is_empty() {
        write!(out, "{OSC8_START}{}{OSC8_MID}{TN_CYAN}#{}{RESET}{OSC8_END}", pr.url, pr.number).unwrap_or_default();
    } else {
        write!(out, "{TN_CYAN}#{}{RESET}", pr.number).unwrap_or_default();
    }

    // State with color (case-insensitive match, display lowercase)
    let state_lower = pr.state.to_lowercase();
    let state_color = match state_lower.as_str() {
        "open" => TN_GREEN,
        "merged" => TN_PURPLE,
        "closed" => TN_RED,
        _ => TN_GRAY,
    };
    write!(out, "{SEP}{state_color}{}{RESET}", state_lower).unwrap_or_default();

    // Comments (if any)
    if pr.comments > 0 {
        write!(out, "{SEP}{TN_GRAY}{} comments{RESET}", pr.comments).unwrap_or_default();
    }

    // Changed files
    if pr.changed_files > 0 {
        write!(out, "{SEP}{TN_GRAY}{} files{RESET}", pr.changed_files).unwrap_or_default();
    }

    // Check status (only show if we have a valid status)
    match pr.check_status.trim() {
        "passed" => write!(out, "{SEP}{TN_GREEN}checks passed{RESET}").unwrap_or_default(),
        "failed" => write!(out, "{SEP}{TN_RED}checks failed{RESET}").unwrap_or_default(),
        "pending" => write!(out, "{SEP}{TN_ORANGE}checks pending{RESET}").unwrap_or_default(),
        _ => {} // No checks or unknown status - show nothing
    }

    writeln!(out).unwrap_or_default();
}

fn compute_and_cache_git_stats(git: &GitRepo, mtime: u64, oid: &str) -> (u32, u32, u32, u32, u32) {
    let (files_changed, lines_added, lines_deleted) = git.diff_stats().unwrap_or((0, 0, 0));
    let ahead = 0u32; // Simplified - gix ahead/behind is complex
    let behind = 0u32;

    let mut cache = MmapCache::default();
    cache.index_mtime = mtime;
    let oid_bytes = oid.as_bytes();
    let copy_len = oid_bytes.len().min(40);
    cache.head_oid[..copy_len].copy_from_slice(&oid_bytes[..copy_len]);
    cache.files_changed = files_changed;
    cache.lines_added = lines_added;
    cache.lines_deleted = lines_deleted;
    cache.ahead = ahead;
    cache.behind = behind;
    save_mmap_cache(&git.git_dir, &cache);

    (files_changed, lines_added, lines_deleted, ahead, behind)
}

fn write_row3<W: Write>(out: &mut W, data: &ClaudeInput) {
    let mut has_content = false;

    if let Some(model) = &data.model.display_name {
        if model != "Unknown" {
            write!(out, "{TN_ORANGE}{model}{RESET}").unwrap_or_default();
            has_content = true;
        }
    }

    let context_pct = data.context_window.remaining_percentage.unwrap_or(100.0) as u32;
    if context_pct < 100 {
        if has_content { write!(out, "{SEP}").unwrap_or_default(); }
        write!(out, "{TN_TEAL}{context_pct}%{RESET}").unwrap_or_default();
        has_content = true;
    }

    if let Some(mode) = &data.output_style.name {
        if mode != "default" {
            if has_content { write!(out, "{SEP}").unwrap_or_default(); }
            write!(out, "{TN_BLUE}{mode}{RESET}").unwrap_or_default();
            has_content = true;
        }
    }

    if has_content {
        writeln!(out).unwrap_or_default();
    }
}

fn write_row4<W: Write>(out: &mut W, data: &ClaudeInput) {
    let mut has_content = false;

    let duration_ms = data.cost.total_duration_ms.unwrap_or(0);
    if duration_ms > 0 {
        let total_secs = duration_ms / 1000;
        let mins = total_secs / 60;
        let hours = mins / 60;
        let mins = mins % 60;

        if hours > 0 {
            write!(out, "{TN_GRAY}{}h {}m{RESET}", hours, mins).unwrap_or_default();
        } else {
            write!(out, "{TN_GRAY}{}m{RESET}", mins).unwrap_or_default();
        }
        has_content = true;
    }

    let input_tokens = data.context_window.total_input_tokens.unwrap_or(0);
    let output_tokens = data.context_window.total_output_tokens.unwrap_or(0);
    if input_tokens > 0 || output_tokens > 0 {
        if has_content { write!(out, "{SEP}").unwrap_or_default(); }
        write!(out, "{TN_GRAY}").unwrap_or_default();
        write_tokens(out, input_tokens);
        write!(out, "/").unwrap_or_default();
        write_tokens(out, output_tokens);
        write!(out, "{RESET}").unwrap_or_default();
        has_content = true;
    }

    if has_content {
        writeln!(out).unwrap_or_default();
    }
}

fn write_tokens<W: Write>(out: &mut W, n: u64) {
    if n >= 1_000_000 {
        let tenths = n / 100_000;
        let whole = tenths / 10;
        let frac = tenths % 10;
        let _ = write!(out, "{}.{}M", whole, frac);
    } else if n >= 1_000 {
        let _ = write!(out, "{}K", n / 1_000);
    } else {
        let _ = write!(out, "{}", n);
    }
}
