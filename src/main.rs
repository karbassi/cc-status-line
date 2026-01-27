use cc_statusline::{abbreviate_path, hash_path, parse_github_url, percent_encode, shell_escape};
use gix::Repository;
use memmap2::{MmapMut, MmapOptions};
use serde::Deserialize;
use std::borrow::Cow;
use std::env;
use std::fs::{self, OpenOptions};
use std::io::{self, BufWriter, Read, Write};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::OnceLock;
use std::time::SystemTime;

static HOME_DIR: OnceLock<String> = OnceLock::new();
static CACHE_DIR: OnceLock<PathBuf> = OnceLock::new();
static GH_AVAILABLE: OnceLock<bool> = OnceLock::new();

fn get_home() -> &'static str {
    HOME_DIR.get_or_init(|| {
        // Try HOME first (Unix standard), then USERPROFILE (Windows standard)
        env::var("HOME")
            .or_else(|_| env::var("USERPROFILE"))
            .unwrap_or_default()
    })
}

/// Get secure per-user cache directory
/// Uses $XDG_CACHE_HOME/cc-statusline or ~/.cache/cc-statusline
fn get_cache_dir() -> &'static PathBuf {
    CACHE_DIR.get_or_init(|| {
        let base = env::var("XDG_CACHE_HOME").map_or_else(
            |_| {
                let home = get_home();
                if home.is_empty() {
                    // Fallback to system temp dir with user-specific subdirectory
                    // Use std::env::temp_dir() for portability
                    let mut base = env::temp_dir();
                    #[cfg(unix)]
                    let uid = unsafe { libc::getuid() };
                    #[cfg(not(unix))]
                    let uid = std::process::id();
                    base.push(format!("cc-statusline-{uid}"));
                    base
                } else {
                    PathBuf::from(home).join(".cache")
                }
            },
            PathBuf::from,
        );
        let cache_dir = base.join("cc-statusline");
        // Create directory with restricted permissions (0700)
        let _ = fs::create_dir_all(&cache_dir);
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let _ = fs::set_permissions(&cache_dir, fs::Permissions::from_mode(0o700));
            // Security: verify the directory is owned by us (defense against pre-creation attacks)
            // If ownership check fails, the directory may have been pre-created by an attacker
            if let Ok(metadata) = fs::metadata(&cache_dir) {
                use std::os::unix::fs::MetadataExt;
                let dir_uid = metadata.uid();
                let our_uid = unsafe { libc::getuid() };
                if dir_uid != our_uid {
                    // Directory not owned by us - try a per-user temp directory
                    let mut fallback_dir = env::temp_dir();
                    fallback_dir.push(format!("cc-statusline-{our_uid}"));
                    let _ = fs::create_dir_all(&fallback_dir);
                    let _ = fs::set_permissions(&fallback_dir, fs::Permissions::from_mode(0o700));

                    // Verify the fallback is owned by us
                    if let Ok(fb_meta) = fs::metadata(&fallback_dir)
                        && fb_meta.is_dir()
                        && fb_meta.uid() == our_uid
                    {
                        return fallback_dir;
                    }
                    // If no safe directory can be created, disable caching
                    // Use a path that will fail gracefully on file operations
                    return PathBuf::from("/dev/null");
                }
            }
        }
        cache_dir
    })
}

/// Check if gh CLI is available (cached)
fn is_gh_available() -> bool {
    *GH_AVAILABLE.get_or_init(|| {
        Command::new("gh")
            .arg("--version")
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .status()
            .map(|s| s.success())
            .unwrap_or(false)
    })
}

/// Get GitHub token for API authentication
/// Tries: 1) `GITHUB_TOKEN` env var, 2) `GH_TOKEN` env var, 3) git credential fill
fn get_github_token() -> Option<String> {
    // Try GITHUB_TOKEN env first
    if let Ok(token) = env::var("GITHUB_TOKEN")
        && !token.is_empty()
    {
        return Some(token);
    }

    // Try GH_TOKEN (used by gh CLI)
    if let Ok(token) = env::var("GH_TOKEN")
        && !token.is_empty()
    {
        return Some(token);
    }

    // Try git credential helper
    let mut child = Command::new("git")
        .args(["credential", "fill"])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .ok()?;

    // Write credential request to stdin
    if let Some(mut stdin) = child.stdin.take() {
        let _ = writeln!(stdin, "protocol=https");
        let _ = writeln!(stdin, "host=github.com");
        let _ = writeln!(stdin);
    }

    // Parse password from output
    let output = child.wait_with_output().ok()?;
    let stdout = String::from_utf8_lossy(&output.stdout);
    for line in stdout.lines() {
        if let Some(token) = line.strip_prefix("password=") {
            return Some(token.to_string());
        }
    }
    None
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

/// Best-effort cross-platform rename that overwrites the destination.
///
/// On Unix-like platforms this is typically atomic. On Windows, `fs::rename`
/// fails if the destination exists, so we remove the destination first and
/// then rename. This is *not* a truly atomic replacement on Windows, as
/// there is a brief window where the destination path does not exist.
fn atomic_rename(from: &Path, to: &Path) -> io::Result<()> {
    // On Windows, fs::rename fails if destination exists; remove it first.
    #[cfg(windows)]
    let _ = fs::remove_file(to);
    fs::rename(from, to)
}

#[derive(Deserialize, Default)]
#[serde(default)]
struct ClaudeInput {
    model: Model,
    context_window: ContextWindow,
    cost: Cost,
    output_style: OutputStyle,
    workspace: Workspace,
    git: GitInput,
    pr: PrInput,
}

#[derive(Deserialize, Default)]
#[serde(default)]
struct Model {
    display_name: Option<String>,
}

#[derive(Deserialize, Default)]
#[serde(default)]
struct ContextWindow {
    remaining_percentage: Option<f64>,
    total_input_tokens: Option<u64>,
    total_output_tokens: Option<u64>,
}

#[derive(Deserialize, Default)]
#[serde(default)]
struct Cost {
    total_duration_ms: Option<u64>,
}

#[derive(Deserialize, Default)]
#[serde(default)]
struct OutputStyle {
    name: Option<String>,
}

#[derive(Deserialize, Default)]
#[serde(default)]
struct Workspace {
    project_dir: Option<String>,
    current_dir: Option<String>,
}

/// Git info from JSON input (for screenshots/testing)
#[derive(Deserialize, Default)]
#[serde(default)]
struct GitInput {
    branch: Option<String>,
    worktree: Option<String>,
    changed_files: Option<u32>,
    ahead: Option<u32>,
    behind: Option<u32>,
}

/// PR info from JSON input (for screenshots/testing)
#[derive(Deserialize, Default)]
#[serde(default)]
struct PrInput {
    number: Option<u32>,
    state: Option<String>,
    url: Option<String>,
    comments: Option<u32>,
    changed_files: Option<u32>,
    check_status: Option<String>,
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

        let mut head_oid = [0u8; 40];
        head_oid.copy_from_slice(&data[16..56]);
        Some(MmapCache {
            index_mtime: u64::from_le_bytes(data[8..16].try_into().ok()?),
            head_oid,
            files_changed: u32::from_le_bytes(data[56..60].try_into().ok()?),
            lines_added: u32::from_le_bytes(data[60..64].try_into().ok()?),
            lines_deleted: u32::from_le_bytes(data[64..68].try_into().ok()?),
            ahead: u32::from_le_bytes(data[68..72].try_into().ok()?),
            behind: u32::from_le_bytes(data[72..76].try_into().ok()?),
        })
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

/// JSON structure from gh pr view (or native API cache)
/// Supports both gh CLI format (comments as array) and native format (commentsCount as number)
#[derive(Deserialize, Default)]
struct GhPrJson {
    number: Option<u64>,
    state: Option<String>,
    url: Option<String>,
    /// gh CLI returns array, native API stores count directly
    comments: Option<Vec<serde_json::Value>>,
    /// Native API stores count directly (preferred, avoids large array allocation)
    #[serde(rename = "commentsCount")]
    comments_count: Option<u64>,
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
const PR_NEGATIVE_CACHE_TTL: u64 = 300; // 5 minutes for "no PR" cache
const PR_REFRESH_THROTTLE: u64 = 30; // minimum seconds between refresh attempts

/// Result of loading PR cache - handles all states in one read
enum PrCacheResult {
    Hit(PrCacheData), // Valid PR data
    NoPr,             // Negative cache: no PR exists for this branch
    Stale,            // Cache is stale or error occurred, needs refresh
}

fn get_pr_cache_path(repo_path: &str, branch: &str) -> PathBuf {
    let key = format!("{repo_path}:{branch}");
    get_cache_dir().join(format!("pr-{:016x}.cache", hash_path(&key)))
}

fn get_pr_attempt_path(repo_path: &str, branch: &str) -> PathBuf {
    let key = format!("{repo_path}:{branch}");
    get_cache_dir().join(format!("pr-attempt-{:016x}", hash_path(&key)))
}

/// Load PR cache - reads file once and handles all states
fn load_pr_cache(repo_path: &str, branch: &str) -> PrCacheResult {
    let cache_path = get_pr_cache_path(repo_path, branch);
    let Ok(content) = fs::read_to_string(&cache_path) else {
        return PrCacheResult::Stale;
    };

    // Cache file format:
    //   1st line: UNIX timestamp (seconds since epoch)
    //   2nd line: cached branch name
    //   remaining lines: JSON payload, "NO_PR" marker, or "ERROR:..." marker
    let mut lines = content.lines();
    let timestamp: u64 = match lines.next().and_then(|s| s.parse().ok()) {
        Some(t) => t,
        None => return PrCacheResult::Stale,
    };
    let Some(cached_branch) = lines.next() else {
        return PrCacheResult::Stale;
    };

    // Validate branch matches
    if cached_branch != branch {
        let _ = fs::remove_file(&cache_path);
        return PrCacheResult::Stale;
    }

    let now = SystemTime::now()
        .duration_since(SystemTime::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0);
    let age = now.saturating_sub(timestamp);

    // Rest is JSON - check for special markers first
    let json_str: String = lines.collect::<Vec<_>>().join("\n");

    // Handle NO_PR marker (negative cache with longer TTL)
    if json_str == "NO_PR" {
        if age < PR_NEGATIVE_CACHE_TTL {
            return PrCacheResult::NoPr;
        }
        return PrCacheResult::Stale;
    }

    // Handle ERROR marker - don't cache errors, always retry
    if json_str.starts_with("ERROR:") {
        return PrCacheResult::Stale;
    }

    // Check normal TTL
    if age > PR_CACHE_TTL {
        return PrCacheResult::Stale;
    }

    // Parse JSON
    let pr: GhPrJson = match serde_json::from_str(&json_str) {
        Ok(p) => p,
        Err(_) => return PrCacheResult::Stale,
    };

    // Compute check status from rollup
    // Note: gh CLI returns uppercase (SUCCESS), REST API returns lowercase (success)
    let check_status = match &pr.status_check_rollup {
        None => String::new(),
        Some(checks) if checks.is_empty() => String::new(),
        Some(checks) => {
            // Case-insensitive check for passing conclusions
            let is_passing = |s: &str| {
                matches!(
                    s.to_ascii_uppercase().as_str(),
                    "SUCCESS" | "SKIPPED" | "NEUTRAL"
                )
            };

            // Treat any non-success conclusion as a failure
            let has_failure = checks.iter().any(|c| {
                match c.conclusion.as_deref() {
                    Some(conc) if is_passing(conc) => false,
                    Some(_) => true, // FAILURE, CANCELLED, TIMED_OUT, ACTION_REQUIRED, etc.
                    None => false,
                }
            });
            let has_pending = checks.iter().any(|c| c.conclusion.is_none());
            let all_passed = checks.iter().all(|c| match c.conclusion.as_deref() {
                Some(conc) => is_passing(conc),
                None => false,
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

    // Validate required fields - treat missing/invalid data as stale
    #[allow(clippy::cast_possible_truncation)] // PR numbers/counts won't exceed u32::MAX
    let number = match pr.number {
        Some(n) if n > 0 => n as u32,
        _ => return PrCacheResult::Stale,
    };

    let state = match pr.state {
        Some(s) if !s.is_empty() => s,
        _ => return PrCacheResult::Stale,
    };

    let url = match pr.url {
        Some(u) if !u.is_empty() => u,
        _ => return PrCacheResult::Stale,
    };

    // Prefer commentsCount (numeric) over comments array to avoid large allocations
    #[allow(clippy::cast_possible_truncation)] // PR numbers/counts won't exceed u32::MAX
    let comments = pr
        .comments_count
        .map(|c| c as u32)
        .or_else(|| pr.comments.map(|c| c.len() as u32))
        .unwrap_or(0);

    #[allow(clippy::cast_possible_truncation)] // PR numbers/counts won't exceed u32::MAX
    PrCacheResult::Hit(PrCacheData {
        number,
        state,
        url,
        comments,
        changed_files: pr.changed_files.unwrap_or(0) as u32,
        check_status,
    })
}

// ============================================================================
// PR Fetch (background only)
// ============================================================================

/// Check if remote is GitHub
/// Delegates to `parse_github_remote` which validates the origin URL as GitHub
fn is_github_remote(git_dir: &str) -> bool {
    parse_github_remote(git_dir).is_some()
}

/// Parse GitHub owner/repo from git remote URL
/// Handles: git@github.com:owner/repo.git, <https://github.com/owner/repo.git>
fn parse_github_remote(git_dir: &str) -> Option<(String, String)> {
    // Use gix to get the common dir (handles worktrees automatically)
    let common_dir = gix::open(git_dir).ok().map_or_else(
        || Path::new(git_dir).to_path_buf(),
        |repo| repo.common_dir().to_path_buf(),
    );

    let config_path = common_dir.join("config");
    let content = fs::read_to_string(&config_path).ok()?;

    // Find origin remote URL
    let mut in_origin_section = false;
    for line in content.lines() {
        let line = line.trim();
        if line.starts_with('[') {
            in_origin_section = line == "[remote \"origin\"]";
            continue;
        }
        // Handle various whitespace: "url = ", "url= ", "url=", "\turl = ", etc.
        if in_origin_section
            && let Some(url) = line
                .strip_prefix("url")
                .and_then(|s| s.trim_start().strip_prefix('='))
                .map(str::trim)
        {
            return parse_github_url(url);
        }
    }
    None
}

/// Generate a unique hex string for temp file names
/// Uses timestamp + pid + atomic counter to avoid collisions within same process
fn unique_hex() -> String {
    use std::sync::atomic::{AtomicU64, Ordering};
    use std::time::{SystemTime, UNIX_EPOCH};
    static COUNTER: AtomicU64 = AtomicU64::new(0);

    #[allow(clippy::cast_possible_truncation)] // Truncation is fine for uniqueness
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_nanos() as u64)
        .unwrap_or(0);
    let pid = std::process::id();
    let count = COUNTER.fetch_add(1, Ordering::Relaxed);
    format!("{nanos:016x}{pid:08x}{count:04x}")
}

/// Spawn background process to refresh PR cache using gh CLI
/// Uses atomic writes: write to temp file, then rename
/// Distinguishes "no PR" from gh errors to avoid false negative caching
/// Only available on Unix (requires sh shell)
#[cfg(unix)]
fn spawn_pr_refresh_gh(git_dir: &str, work_dir: &str, branch: &str) {
    let cache_path = get_pr_cache_path(git_dir, branch);
    let cache_path_str = cache_path.to_string_lossy();
    let now = SystemTime::now()
        .duration_since(SystemTime::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0);

    // Create temp files with random suffix in secure cache directory
    let random_suffix = unique_hex();
    let temp_cache = get_cache_dir().join(format!("pr-tmp-{random_suffix}.cache"));
    let temp_cache_str = temp_cache.to_string_lossy();
    let script_path = get_cache_dir().join(format!("pr-refresh-{random_suffix}.sh"));

    // Script logic:
    // 1. Run gh pr view and capture stdout/stderr separately
    // 2. If gh succeeds with JSON output -> write PR data
    // 3. If gh fails with "no pull requests" message -> write NO_PR (legitimate no PR)
    // 4. If gh fails for other reasons -> write ERROR (don't negative cache)
    // 5. Atomic rename temp file to cache file
    // Uses trap with $0 for cleanup to avoid quoting issues with shell_escape
    let script = format!(
        r#"#!/bin/sh
trap 'rm -f "$0"' EXIT
cd {work_dir} || exit 1
# Capture stdout and stderr separately to detect "no PR" vs other errors
json=$(gh pr view --json number,state,url,comments,changedFiles,statusCheckRollup 2>/dev/null)
exit_code=$?
if [ $exit_code -eq 0 ] && [ -n "$json" ]; then
    # Success with JSON output - PR exists
    printf '%s\n%s\n%s' {timestamp} {branch} "$json" > {temp_cache}
    mv -f {temp_cache} {cache_path}
elif [ $exit_code -ne 0 ]; then
    # gh failed - check if it's "no PR" error by running again and capturing stderr only
    # Use file descriptor swap: redirect stdout to /dev/null first, then capture stderr
    err=$(gh pr view 2>&1 1>/dev/null)
    case "$err" in
        *"no pull requests"*|*"no open pull requests"*|*"Could not resolve to a PullRequest"*)
            # Legitimate "no PR" - negative cache
            printf '%s\n%s\nNO_PR' {timestamp} {branch} > {temp_cache}
            mv -f {temp_cache} {cache_path}
            ;;
        *)
            # Other error (auth, network, etc) - don't negative cache
            printf '%s\n%s\nERROR:%s' {timestamp} {branch} "$err" > {temp_cache}
            mv -f {temp_cache} {cache_path}
            ;;
    esac
fi
"#,
        work_dir = shell_escape(work_dir),
        timestamp = now,
        branch = shell_escape(branch),
        temp_cache = shell_escape(&temp_cache_str),
        cache_path = shell_escape(&cache_path_str),
    );

    if fs::write(&script_path, &script).is_err() {
        return;
    }

    // Set executable permission
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let _ = fs::set_permissions(&script_path, fs::Permissions::from_mode(0o700));
    }

    let _ = Command::new("sh")
        .arg(&script_path)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn();
}

/// Refresh PR cache using native HTTP (synchronous)
/// Works on all platforms, no gh CLI required
/// Note: Runs synchronously because threads don't survive process exit.
/// First call may be slow (~500ms), but throttling ensures subsequent calls use cache.
fn refresh_pr_native(git_dir: &str, branch: &str) {
    // Get owner/repo from remote URL
    let Some((owner, repo)) = parse_github_remote(git_dir) else {
        return;
    };

    // Get auth token (may block on git credential helper)
    let Some(token) = get_github_token() else {
        return; // No auth, skip PR feature
    };

    fetch_pr_data_native(git_dir, branch, &owner, &repo, &token);
}

/// Fetch PR data using native HTTP (ureq)
#[allow(clippy::too_many_lines)]
fn fetch_pr_data_native(git_dir: &str, branch: &str, owner: &str, repo: &str, token: &str) {
    let now = SystemTime::now()
        .duration_since(SystemTime::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0);

    let cache_path = get_pr_cache_path(git_dir, branch);

    // GitHub API: GET /repos/{owner}/{repo}/pulls?head={owner}:{branch}&state=all
    // Use state=all to show merged/closed PRs too (not just open)
    // URL-encode the branch name to handle special characters like # or spaces
    let encoded_branch = percent_encode(branch);
    let url = format!(
        "https://api.github.com/repos/{owner}/{repo}/pulls?head={owner}:{encoded_branch}&state=all"
    );

    let response = ureq::get(&url)
        .set("Authorization", &format!("Bearer {token}"))
        .set("Accept", "application/vnd.github+json")
        .set("User-Agent", "cc-statusline")
        .set("X-GitHub-Api-Version", "2022-11-28")
        .call();

    let cache_content = match response {
        Ok(resp) => {
            let Ok(body) = resp.into_string() else {
                return;
            };

            // Parse as array of PRs
            let prs: Vec<serde_json::Value> = match serde_json::from_str(&body) {
                Ok(p) => p,
                Err(_) => return,
            };

            if prs.is_empty() {
                // No PR for this branch - negative cache
                format!("{now}\n{branch}\nNO_PR")
            } else {
                // Found PR - convert to gh-compatible format
                let pr = &prs[0];
                let pr_number = pr["number"].as_u64().unwrap_or(0);
                let pr_url = pr["html_url"].as_str().unwrap_or("");

                // Fetch additional PR details (comments, check status)
                let detail_url =
                    format!("https://api.github.com/repos/{owner}/{repo}/pulls/{pr_number}");
                let detail_resp = ureq::get(&detail_url)
                    .set("Authorization", &format!("Bearer {token}"))
                    .set("Accept", "application/vnd.github+json")
                    .set("User-Agent", "cc-statusline")
                    .set("X-GitHub-Api-Version", "2022-11-28")
                    .call();

                let (comments_count, changed_files) = match detail_resp {
                    Ok(resp) => {
                        let body = resp.into_string().unwrap_or_default();
                        let detail: serde_json::Value =
                            serde_json::from_str(&body).unwrap_or_default();
                        (
                            detail["comments"].as_u64().unwrap_or(0)
                                + detail["review_comments"].as_u64().unwrap_or(0),
                            detail["changed_files"].as_u64().unwrap_or(0),
                        )
                    }
                    Err(_) => (0, 0),
                };

                // Fetch check runs status
                let checks_url = format!(
                    "https://api.github.com/repos/{}/{}/commits/{}/check-runs",
                    owner,
                    repo,
                    pr["head"]["sha"].as_str().unwrap_or("")
                );
                let checks_resp = ureq::get(&checks_url)
                    .set("Authorization", &format!("Bearer {token}"))
                    .set("Accept", "application/vnd.github+json")
                    .set("User-Agent", "cc-statusline")
                    .set("X-GitHub-Api-Version", "2022-11-28")
                    .call();

                let check_rollup: Vec<serde_json::Value> = match checks_resp {
                    Ok(resp) => {
                        let body = resp.into_string().unwrap_or_default();
                        let checks: serde_json::Value =
                            serde_json::from_str(&body).unwrap_or_default();
                        checks["check_runs"]
                            .as_array()
                            .map(|runs| {
                                runs.iter()
                                    .map(|run| {
                                        serde_json::json!({
                                            "conclusion": run["conclusion"]
                                        })
                                    })
                                    .collect()
                            })
                            .unwrap_or_default()
                    }
                    Err(_) => vec![],
                };

                // Build cache JSON - use commentsCount (number) instead of comments array
                // to avoid large allocations when deserializing
                let gh_json = serde_json::json!({
                    "number": pr_number,
                    "state": pr["state"],
                    "url": pr_url,
                    "commentsCount": comments_count,
                    "changedFiles": changed_files,
                    "statusCheckRollup": check_rollup
                });

                format!("{now}\n{branch}\n{gh_json}")
            }
        }
        Err(ureq::Error::Status(code, _)) => {
            // API error (401/403/404 etc) - don't negative cache
            // Note: 404 can mean "no access" for private repos, not just "no PR"
            format!("{now}\n{branch}\nERROR:HTTP {code}")
        }
        Err(e) => {
            // Network error - don't negative cache
            format!("{now}\n{branch}\nERROR:{e}")
        }
    };

    // Atomic write to cache
    let temp_path = get_cache_dir().join(format!("pr-tmp-{}.cache", unique_hex()));
    if fs::write(&temp_path, &cache_content).is_ok() {
        let _ = atomic_rename(&temp_path, &cache_path);
    }
}

/// Dispatch PR refresh to appropriate implementation
/// Returns true if refresh was synchronous (cache can be re-read immediately)
fn spawn_pr_refresh(git_dir: &str, work_dir: &str, branch: &str) -> bool {
    // Only proceed if this is a GitHub repo
    if !is_github_remote(git_dir) {
        return false;
    }

    // On Unix, prefer gh if available (handles auth, rate limits better)
    #[cfg(unix)]
    if is_gh_available() {
        spawn_pr_refresh_gh(git_dir, work_dir, branch);
        return false; // Background process, cache not ready yet
    }

    // Fallback to native HTTP (works on all platforms, no gh required)
    refresh_pr_native(git_dir, branch);
    true // Synchronous, cache is ready
}

/// Check if we should skip refresh (throttled or negative cache)
fn should_skip_refresh(git_dir: &str, branch: &str) -> bool {
    let attempt_path = get_pr_attempt_path(git_dir, branch);
    if let Ok(metadata) = fs::metadata(&attempt_path)
        && let Ok(mtime) = metadata.modified()
    {
        let now = SystemTime::now();
        if let Ok(elapsed) = now.duration_since(mtime) {
            // Skip if we attempted recently
            return elapsed.as_secs() < PR_REFRESH_THROTTLE;
        }
    }
    false
}

/// Mark that we've attempted a refresh
fn mark_refresh_attempt(git_dir: &str, branch: &str) {
    let attempt_path = get_pr_attempt_path(git_dir, branch);
    // Atomic write (Windows-compatible)
    let temp_path = get_cache_dir().join(format!("pr-attempt-tmp-{}", unique_hex()));
    if fs::write(&temp_path, "").is_ok() {
        let _ = atomic_rename(&temp_path, &attempt_path);
    }
}

/// Get PR data - checks cache first, triggers refresh if needed
/// On Unix with gh CLI: spawns background process (non-blocking)
/// On other platforms or without gh: runs synchronous HTTP refresh (may block ~500ms)
fn get_pr_data(git: &GitRepo) -> Option<PrCacheData> {
    // Single cache read handles all states
    match load_pr_cache(&git.git_dir, &git.branch) {
        PrCacheResult::Hit(data) => return Some(data),
        PrCacheResult::NoPr => return None, // Negative cache hit - no PR exists
        PrCacheResult::Stale => {}          // Continue to refresh
    }

    // Throttle refresh attempts to avoid process storms
    if should_skip_refresh(&git.git_dir, &git.branch) {
        return None;
    }

    // Mark that we're attempting a refresh
    mark_refresh_attempt(&git.git_dir, &git.branch);

    // Trigger refresh - returns true if synchronous (native path)
    let was_synchronous = spawn_pr_refresh(&git.git_dir, &git.work_dir, &git.branch);

    // If refresh was synchronous, re-read cache to return data immediately
    // This avoids blocking on HTTP but still not showing PR data until next render
    if was_synchronous && let PrCacheResult::Hit(data) = load_pr_cache(&git.git_dir, &git.branch) {
        return Some(data);
    }

    None
}

/// Holds repository state for lazy evaluation of expensive git operations
struct GitRepo {
    repo: Repository,
    branch: String,
    worktree: Option<String>,
    git_dir: String,
    work_dir: String,
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
                let mtime = metadata
                    .modified()
                    .ok()?
                    .duration_since(SystemTime::UNIX_EPOCH)
                    .ok()?
                    .as_secs();
                let index_mtime = u64::from(entry.stat.mtime.secs);

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
            .map(|t| {
                t.duration_since(SystemTime::UNIX_EPOCH)
                    .unwrap_or_default()
                    .as_secs()
            })
            .unwrap_or(0)
    }

    /// Get HEAD oid for cache invalidation
    fn head_oid(&self) -> String {
        let ref_path = format!(
            "{}/refs/heads/{}",
            self.git_dir.trim_end_matches('/'),
            self.branch
        );
        if let Ok(oid) = fs::read_to_string(&ref_path) {
            return oid.trim().to_string();
        }
        self.repo
            .head_id()
            .map(|id| id.to_string())
            .unwrap_or_default()
    }
}

fn get_cache_path(git_dir: &str) -> PathBuf {
    get_cache_dir().join(format!("status-{:016x}.cache", hash_path(git_dir)))
}

fn load_mmap_cache(git_dir: &str) -> Option<MmapCache> {
    let cache_path = get_cache_path(git_dir);
    let file = OpenOptions::new().read(true).open(&cache_path).ok()?;
    let mmap = unsafe { MmapOptions::new().map(&file).ok()? };
    MmapCache::from_bytes(&mmap)
}

fn save_mmap_cache(git_dir: &str, cache: &MmapCache) {
    let cache_path = get_cache_path(git_dir);
    // Atomic write: write to temp file, then rename
    let temp_path = get_cache_dir().join(format!("status-tmp-{}.cache", unique_hex()));

    let Ok(file) = OpenOptions::new()
        .read(true)
        .write(true)
        .create(true)
        .truncate(true)
        .open(&temp_path)
    else {
        return;
    };
    if file.set_len(CACHE_SIZE as u64).is_err() {
        let _ = fs::remove_file(&temp_path);
        return;
    }
    let Ok(mut mmap) = (unsafe { MmapMut::map_mut(&file) }) else {
        let _ = fs::remove_file(&temp_path);
        return;
    };
    cache.to_bytes(&mut mmap);
    if mmap.flush().is_err() {
        let _ = fs::remove_file(&temp_path);
        return;
    }
    drop(mmap);
    drop(file);
    let _ = atomic_rename(&temp_path, &cache_path);
}

struct GitPathCache {
    git_path: String,
    branch: String,
}

fn get_head_mtime(git_path: &str) -> u64 {
    let head_path = format!("{}/HEAD", git_path.trim_end_matches('/'));
    fs::metadata(&head_path)
        .and_then(|m| m.modified())
        .map(|t| {
            t.duration_since(SystemTime::UNIX_EPOCH)
                .unwrap_or_default()
                .as_secs()
        })
        .unwrap_or(0)
}

fn get_cached_git_info(working_dir: &str) -> Option<GitPathCache> {
    let cache_path = get_cache_dir().join(format!("gitpath-{:016x}.cache", hash_path(working_dir)));
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
    let cache_path = get_cache_dir().join(format!("gitpath-{:016x}.cache", hash_path(working_dir)));
    let head_mtime = get_head_mtime(git_path);
    let content = format!("{git_path}\n{branch}\n{head_mtime}");
    // Atomic write (Windows-compatible): write to temp, then rename
    let temp_path = get_cache_dir().join(format!("gitpath-tmp-{}.cache", unique_hex()));
    if fs::write(&temp_path, &content).is_ok() {
        let _ = atomic_rename(&temp_path, &cache_path);
    }
}

fn main() {
    let mut input = String::with_capacity(4096);
    io::stdin().read_to_string(&mut input).unwrap_or_default();

    let data: ClaudeInput = serde_json::from_str(&input).unwrap_or_default();

    let current_dir: Cow<str> = match data.workspace.current_dir.as_deref() {
        Some(dir) => Cow::Borrowed(dir),
        None => match data.workspace.project_dir.as_deref() {
            Some(dir) => Cow::Borrowed(dir), // Default to project_dir if current_dir not set
            None => Cow::Owned(env::current_dir().unwrap().to_string_lossy().into_owned()),
        },
    };

    let stdout = io::stdout();
    let mut out = BufWriter::new(stdout.lock());

    write_row1(&mut out, &data, &current_dir);

    // Skip filesystem detection if JSON provides git.branch
    let git_repo = if data.git.branch.is_some() {
        None
    } else {
        get_git_repo(&current_dir)
    };

    write_row2(&mut out, git_repo.as_ref(), &data.git);
    write_pr_rows(&mut out, git_repo.as_ref(), &data.pr);
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
        Cow::Borrowed(current_dir)
    };

    let path_width = TERM_WIDTH
        .saturating_sub(project_name.len())
        .saturating_sub(3)
        .max(10);
    let abbrev_cwd = abbreviate_path(&display_cwd, path_width);

    writeln!(
        out,
        "{TN_BLUE}{project_name}{RESET}{SEP}{TN_CYAN}{abbrev_cwd}{RESET}"
    )
    .unwrap_or_default();
}

/// Detect linked worktree name from `git_dir` path
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
        let work_dir = repo
            .work_dir()
            .map_or_else(|| dir.to_string(), |p| p.to_string_lossy().into_owned());
        let worktree = get_worktree_name(&cache.git_path);
        return Some(GitRepo {
            repo,
            branch: cache.branch,
            worktree,
            git_dir: cache.git_path,
            work_dir,
        });
    }

    // Discover repo
    let repo = gix::discover(dir).ok()?;
    let git_dir = repo.git_dir().to_string_lossy().into_owned();
    let work_dir = repo
        .work_dir()
        .map_or_else(|| dir.to_string(), |p| p.to_string_lossy().into_owned());

    // Get branch name from HEAD
    let head = repo.head().ok()?;
    let branch = head
        .referent_name()
        .map_or_else(|| "HEAD".to_string(), |n| n.shorten().to_string());

    let worktree = get_worktree_name(&git_dir);

    cache_git_info(dir, &git_dir, &branch);
    Some(GitRepo {
        repo,
        branch,
        worktree,
        git_dir,
        work_dir,
    })
}

fn write_row2<W: Write>(out: &mut W, git: Option<&GitRepo>, git_input: &GitInput) {
    // Get branch: prefer JSON input, fallback to filesystem detection
    let branch = git_input
        .branch
        .as_deref()
        .or_else(|| git.map(|g| g.branch.as_str()));

    let Some(branch) = branch else {
        writeln!(out, "{TN_GRAY}no git{RESET}").unwrap_or_default();
        return;
    };

    write!(out, "{TN_PURPLE}{branch}{RESET}").unwrap_or_default();

    // Worktree: prefer JSON input, fallback to filesystem
    let worktree = git_input
        .worktree
        .as_deref()
        .or_else(|| git.and_then(|g| g.worktree.as_deref()));
    if let Some(wt) = worktree {
        write!(out, "{SEP}{TN_MAGENTA}{wt}{RESET}").unwrap_or_default();
    }

    // Get stats: prefer JSON input, fallback to cache/detection
    let (files_changed, ahead, behind) = if git_input.branch.is_some() {
        // Using JSON input
        (
            git_input.changed_files.unwrap_or(0),
            git_input.ahead.unwrap_or(0),
            git_input.behind.unwrap_or(0),
        )
    } else if let Some(g) = git {
        // Using filesystem detection
        let cache = load_mmap_cache(&g.git_dir);
        let current_mtime = g.index_mtime();
        let current_oid = g.head_oid();

        let (files, _, _) = if let Some(ref c) = cache {
            if c.index_mtime == current_mtime && c.head_oid_matches(&current_oid) {
                (c.files_changed, c.lines_added, c.lines_deleted)
            } else {
                compute_and_cache_git_stats(g, current_mtime, &current_oid)
            }
        } else {
            compute_and_cache_git_stats(g, current_mtime, &current_oid)
        };

        let (a, b) = get_ahead_behind(&g.repo, &g.branch);
        (files, a, b)
    } else {
        (0, 0, 0)
    };

    if files_changed > 0 {
        write!(out, "{SEP}{TN_GRAY}{files_changed} files{RESET}").unwrap_or_default();
    }

    if ahead > 0 || behind > 0 {
        write!(out, "{SEP}").unwrap_or_default();
        if ahead > 0 {
            write!(out, "{TN_GRAY}↑{ahead}{RESET}").unwrap_or_default();
        }
        if behind > 0 {
            if ahead > 0 {
                write!(out, " ").unwrap_or_default();
            }
            write!(out, "{TN_GRAY}↓{behind}{RESET}").unwrap_or_default();
        }
    }

    writeln!(out).unwrap_or_default();
}

/// Write PR info rows (only shown when a PR exists for current branch)
fn write_pr_rows<W: Write>(out: &mut W, git: Option<&GitRepo>, pr_input: &PrInput) {
    // Get PR data: prefer JSON input, fallback to cache
    let (number, state, url, comments, changed_files, check_status) =
        if let Some(n) = pr_input.number {
            // Using JSON input
            (
                n,
                pr_input.state.clone().unwrap_or_default(),
                pr_input.url.clone().unwrap_or_default(),
                pr_input.comments.unwrap_or(0),
                pr_input.changed_files.unwrap_or(0),
                pr_input.check_status.clone().unwrap_or_default(),
            )
        } else if let Some(g) = git {
            // Using filesystem detection
            let Some(pr) = get_pr_data(g) else { return };
            (
                pr.number,
                pr.state,
                pr.url,
                pr.comments,
                pr.changed_files,
                pr.check_status,
            )
        } else {
            return;
        };

    // PR number (cyan, clickable via OSC 8)
    if url.is_empty() {
        write!(out, "{TN_CYAN}#{number}{RESET}").unwrap_or_default();
    } else {
        write!(
            out,
            "{OSC8_START}{url}{OSC8_MID}{TN_CYAN}#{number}{RESET}{OSC8_END}"
        )
        .unwrap_or_default();
    }

    // State with color (case-insensitive match, display lowercase)
    let state_lower = state.to_lowercase();
    let state_color = match state_lower.as_str() {
        "open" => TN_GREEN,
        "merged" => TN_PURPLE,
        "closed" => TN_RED,
        _ => TN_GRAY,
    };
    write!(out, "{SEP}{state_color}{state_lower}{RESET}").unwrap_or_default();

    // Comments (if any)
    if comments > 0 {
        let label = if comments == 1 { "comment" } else { "comments" };
        write!(out, "{SEP}{TN_GRAY}{comments} {label}{RESET}").unwrap_or_default();
    }

    // Changed files
    if changed_files > 0 {
        let label = if changed_files == 1 { "file" } else { "files" };
        write!(out, "{SEP}{TN_GRAY}{changed_files} {label}{RESET}").unwrap_or_default();
    }

    // Check status (only show if we have a valid status)
    let checks_url = if url.is_empty() {
        String::new()
    } else {
        format!("{url}/checks")
    };
    match check_status.trim() {
        "passed" if !checks_url.is_empty() => write!(
            out,
            "{SEP}{OSC8_START}{checks_url}{OSC8_MID}{TN_GREEN}checks passed{RESET}{OSC8_END}"
        )
        .unwrap_or_default(),
        "failed" if !checks_url.is_empty() => write!(
            out,
            "{SEP}{OSC8_START}{checks_url}{OSC8_MID}{TN_RED}checks failed{RESET}{OSC8_END}"
        )
        .unwrap_or_default(),
        "pending" if !checks_url.is_empty() => write!(
            out,
            "{SEP}{OSC8_START}{checks_url}{OSC8_MID}{TN_ORANGE}checks pending{RESET}{OSC8_END}"
        )
        .unwrap_or_default(),
        "passed" => write!(out, "{SEP}{TN_GREEN}checks passed{RESET}").unwrap_or_default(),
        "failed" => write!(out, "{SEP}{TN_RED}checks failed{RESET}").unwrap_or_default(),
        "pending" => write!(out, "{SEP}{TN_ORANGE}checks pending{RESET}").unwrap_or_default(),
        _ => {}
    }

    writeln!(out).unwrap_or_default();
}

/// Find the configured upstream ref for a branch
/// Reads branch.<name>.remote and branch.<name>.merge from git config
fn find_upstream_ref(repo: &gix::Repository, branch: &str) -> Option<String> {
    let config = repo.config_snapshot();

    // Get branch.<name>.remote (e.g., "origin")
    let remote_key = format!("branch.{branch}.remote");
    let remote = config.string(remote_key.as_str())?;
    let remote = remote.to_string();

    // Get branch.<name>.merge (e.g., "refs/heads/main")
    let merge_key = format!("branch.{branch}.merge");
    let merge_ref = config.string(merge_key.as_str())?;
    let merge_ref = merge_ref.to_string();

    // Convert refs/heads/X to refs/remotes/<remote>/X
    let upstream_branch = merge_ref.strip_prefix("refs/heads/")?;
    Some(format!("refs/remotes/{remote}/{upstream_branch}"))
}

/// Get ahead/behind counts relative to upstream using gix
fn get_ahead_behind(repo: &gix::Repository, branch: &str) -> (u32, u32) {
    // Get HEAD commit
    let Ok(head_id) = repo.head_id() else {
        return (0, 0);
    };

    // Try to find configured upstream for this branch first
    // Falls back to origin/<branch> if no upstream configured
    let upstream_ref =
        find_upstream_ref(repo, branch).unwrap_or_else(|| format!("refs/remotes/origin/{branch}"));

    let upstream_id = match repo.find_reference(&upstream_ref) {
        Ok(r) => match r.into_fully_peeled_id() {
            Ok(id) => id,
            Err(_) => return (0, 0),
        },
        Err(_) => return (0, 0), // No upstream
    };

    // If same commit, no ahead/behind
    if head_id == upstream_id {
        return (0, 0);
    }

    // Count commits reachable from HEAD but not upstream (ahead)
    let ahead = count_commits_not_in(repo, head_id.detach(), upstream_id.detach());
    // Count commits reachable from upstream but not HEAD (behind)
    let behind = count_commits_not_in(repo, upstream_id.detach(), head_id.detach());

    (ahead, behind)
}

/// Count commits reachable from `from` but not from `exclude`
///
/// Note: Uses a 10k commit safety limit to prevent runaway computation in very large repos.
/// In repos with >10k commits between branches, counts may be approximate. This is an
/// intentional trade-off for predictable performance in a status line tool.
fn count_commits_not_in(
    repo: &gix::Repository,
    from: gix::ObjectId,
    exclude: gix::ObjectId,
) -> u32 {
    // First, collect all commits reachable from exclude (the "stop" set)
    let mut exclude_set = std::collections::HashSet::new();
    let Ok(exclude_iter) = repo.rev_walk([exclude]).all() else {
        return 0;
    };
    for info in exclude_iter {
        let Ok(info) = info else { break };
        exclude_set.insert(info.id);
        if exclude_set.len() > 10000 {
            break; // Safety limit
        }
    }

    // Now count commits from `from` that aren't in exclude_set
    // Don't break on first intersection - merges can have commits on both sides
    let Ok(from_iter) = repo.rev_walk([from]).all() else {
        return 0;
    };
    let mut count = 0u32;
    let mut visited = 0u32;
    for info in from_iter {
        let Ok(info) = info else { break };
        visited += 1;
        if !exclude_set.contains(&info.id) {
            count += 1;
        }
        if visited > 10000 {
            break; // Safety limit
        }
    }
    count
}

fn compute_and_cache_git_stats(git: &GitRepo, mtime: u64, oid: &str) -> (u32, u32, u32) {
    let (files_changed, lines_added, lines_deleted) = git.diff_stats().unwrap_or((0, 0, 0));

    let oid_bytes = oid.as_bytes();
    let copy_len = oid_bytes.len().min(40);
    let mut head_oid = [0u8; 40];
    head_oid[..copy_len].copy_from_slice(&oid_bytes[..copy_len]);

    let cache = MmapCache {
        index_mtime: mtime,
        head_oid,
        files_changed,
        lines_added,
        lines_deleted,
        ahead: 0,
        behind: 0,
    };
    save_mmap_cache(&git.git_dir, &cache);

    (files_changed, lines_added, lines_deleted)
}

fn write_row3<W: Write>(out: &mut W, data: &ClaudeInput) {
    let mut has_content = false;

    if let Some(model) = &data.model.display_name
        && model != "Unknown"
    {
        write!(out, "{TN_ORANGE}{model}{RESET}").unwrap_or_default();
        has_content = true;
    }

    #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
    let context_pct = data.context_window.remaining_percentage.unwrap_or(100.0) as u32;
    if context_pct < 100 {
        if has_content {
            write!(out, "{SEP}").unwrap_or_default();
        }
        write!(out, "{TN_TEAL}{context_pct}%{RESET}").unwrap_or_default();
        has_content = true;
    }

    if let Some(mode) = &data.output_style.name
        && mode != "default"
    {
        if has_content {
            write!(out, "{SEP}").unwrap_or_default();
        }
        write!(out, "{TN_BLUE}{mode}{RESET}").unwrap_or_default();
        has_content = true;
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
            write!(out, "{TN_GRAY}{hours}h {mins}m{RESET}").unwrap_or_default();
        } else {
            write!(out, "{TN_GRAY}{mins}m{RESET}").unwrap_or_default();
        }
        has_content = true;
    }

    let input_tokens = data.context_window.total_input_tokens.unwrap_or(0);
    let output_tokens = data.context_window.total_output_tokens.unwrap_or(0);
    if input_tokens > 0 || output_tokens > 0 {
        if has_content {
            write!(out, "{SEP}").unwrap_or_default();
        }
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
        let _ = write!(out, "{whole}.{frac}M");
    } else if n >= 1_000 {
        let _ = write!(out, "{}K", n / 1_000);
    } else {
        let _ = write!(out, "{n}");
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // =========================================================================
    // hash_path tests
    // =========================================================================

    #[test]
    fn hash_path_deterministic() {
        let path = "/home/user/project";
        assert_eq!(hash_path(path), hash_path(path));
    }

    #[test]
    fn hash_path_different_inputs() {
        let path1 = "/home/user/project1";
        let path2 = "/home/user/project2";
        assert_ne!(hash_path(path1), hash_path(path2));
    }

    #[test]
    fn hash_path_empty_string() {
        // Empty string should produce a consistent hash (0 in this case)
        assert_eq!(hash_path(""), 0);
    }

    #[test]
    fn hash_path_similar_paths() {
        // Paths that differ by one character should produce different hashes
        assert_ne!(hash_path("/a/b/c"), hash_path("/a/b/d"));
    }

    // =========================================================================
    // parse_github_url tests
    // =========================================================================

    #[test]
    fn parse_ssh_url() {
        let result = parse_github_url("git@github.com:owner/repo.git");
        assert_eq!(result, Some(("owner".to_string(), "repo".to_string())));
    }

    #[test]
    fn parse_ssh_url_without_git_suffix() {
        // SSH URLs sometimes don't have .git suffix
        let result = parse_github_url("git@github.com:owner/repo");
        assert_eq!(result, Some(("owner".to_string(), "repo".to_string())));
    }

    #[test]
    fn parse_https_url() {
        let result = parse_github_url("https://github.com/owner/repo.git");
        assert_eq!(result, Some(("owner".to_string(), "repo".to_string())));
    }

    #[test]
    fn parse_https_url_without_git_suffix() {
        let result = parse_github_url("https://github.com/owner/repo");
        assert_eq!(result, Some(("owner".to_string(), "repo".to_string())));
    }

    #[test]
    fn parse_http_url() {
        let result = parse_github_url("http://github.com/owner/repo.git");
        assert_eq!(result, Some(("owner".to_string(), "repo".to_string())));
    }

    #[test]
    fn reject_non_github_ssh_urls() {
        assert_eq!(parse_github_url("git@gitlab.com:owner/repo.git"), None);
        assert_eq!(parse_github_url("git@bitbucket.org:owner/repo.git"), None);
    }

    #[test]
    fn reject_non_github_https_urls() {
        assert_eq!(parse_github_url("https://gitlab.com/owner/repo.git"), None);
        assert_eq!(
            parse_github_url("https://bitbucket.org/owner/repo.git"),
            None
        );
    }

    #[test]
    fn reject_malformed_urls() {
        assert_eq!(parse_github_url(""), None);
        assert_eq!(parse_github_url("not-a-url"), None);
        assert_eq!(parse_github_url("git@github.com:"), None);
        assert_eq!(parse_github_url("git@github.com:owner"), None);
        assert_eq!(parse_github_url("https://github.com/"), None);
        assert_eq!(parse_github_url("https://github.com/owner"), None);
    }

    #[test]
    fn reject_github_like_urls() {
        // Ensure we don't match domains that contain "github.com" but aren't exactly it
        assert_eq!(
            parse_github_url("https://notgithub.com/owner/repo.git"),
            None
        );
        assert_eq!(
            parse_github_url("https://github.com.evil.com/owner/repo.git"),
            None
        );
    }

    #[test]
    fn parse_github_url_case_insensitive_https() {
        // HTTPS URLs should be case-insensitive for the host
        let result = parse_github_url("https://GitHub.com/owner/repo.git");
        assert_eq!(result, Some(("owner".to_string(), "repo".to_string())));

        let result = parse_github_url("HTTPS://GITHUB.COM/owner/repo.git");
        assert_eq!(result, Some(("owner".to_string(), "repo".to_string())));
    }

    // =========================================================================
    // abbreviate_path tests
    // =========================================================================

    #[test]
    fn path_within_width_unchanged() {
        let path = "~/short";
        let result = abbreviate_path(path, 50);
        assert_eq!(result.as_ref(), path);
    }

    #[test]
    fn path_abbreviated_correctly() {
        let path = "~/very/long/deeply/nested/path/to/project";
        let result = abbreviate_path(path, 30);
        // Should abbreviate parent directories to first char
        assert!(result.len() <= 35); // Allow some slack
        assert!(result.ends_with("project"));
    }

    #[test]
    fn single_segment_path() {
        let path = "project";
        let result = abbreviate_path(path, 5);
        // Single segment can't be abbreviated further
        assert_eq!(result.as_ref(), path);
    }

    #[test]
    fn root_path() {
        let path = "/";
        let result = abbreviate_path(path, 50);
        assert_eq!(result.as_ref(), path);
    }

    #[test]
    fn two_segment_path() {
        let path = "~/project";
        let result = abbreviate_path(path, 5);
        // Should keep both segments as much as possible
        assert!(result.contains("project"));
    }

    #[test]
    fn tilde_home_preserved() {
        let path = "~/a/b/c/d/project";
        let result = abbreviate_path(path, 20);
        // Tilde should be preserved as first char abbreviation
        assert!(result.starts_with('~'));
    }

    // =========================================================================
    // shell_escape tests
    // =========================================================================

    #[test]
    fn shell_escape_single_quotes() {
        let result = shell_escape("it's a test");
        assert_eq!(result, "'it'\\''s a test'");
    }

    #[test]
    fn shell_escape_empty_string() {
        let result = shell_escape("");
        assert_eq!(result, "''");
    }

    #[test]
    fn shell_escape_no_escape_needed() {
        let result = shell_escape("simple");
        assert_eq!(result, "'simple'");
    }

    #[test]
    fn shell_escape_special_chars() {
        // Special shell characters should be safely escaped inside single quotes
        let result = shell_escape("$HOME && rm -rf /");
        assert_eq!(result, "'$HOME && rm -rf /'");
    }

    #[test]
    fn shell_escape_multiple_quotes() {
        let result = shell_escape("it's Bob's");
        assert_eq!(result, "'it'\\''s Bob'\\''s'");
    }

    // =========================================================================
    // percent_encode tests
    // =========================================================================

    #[test]
    fn percent_encode_spaces() {
        let result = percent_encode("hello world");
        assert_eq!(result, "hello%20world");
    }

    #[test]
    fn percent_encode_special_chars() {
        let result = percent_encode("test#branch");
        assert_eq!(result, "test%23branch");
    }

    #[test]
    fn percent_encode_unreserved_chars_unchanged() {
        // RFC 3986 unreserved: ALPHA / DIGIT / "-" / "." / "_" / "~"
        let result = percent_encode("azAZ09-._~");
        assert_eq!(result, "azAZ09-._~");
    }

    #[test]
    fn percent_encode_slash() {
        let result = percent_encode("path/to/file");
        assert_eq!(result, "path%2Fto%2Ffile");
    }

    #[test]
    fn percent_encode_unicode() {
        let result = percent_encode("日本語");
        // Each UTF-8 byte should be encoded
        assert!(result.contains("%"));
        assert!(!result.contains("日"));
    }

    #[test]
    fn percent_encode_empty() {
        let result = percent_encode("");
        assert_eq!(result, "");
    }

    // =========================================================================
    // MmapCache tests
    // =========================================================================

    #[test]
    fn cache_round_trip() {
        let original = MmapCache {
            index_mtime: 1234567890,
            head_oid: *b"abc123def456abc123def456abc123def4567890",
            files_changed: 42,
            lines_added: 100,
            lines_deleted: 50,
            ahead: 3,
            behind: 5,
        };

        let mut buf = [0u8; CACHE_SIZE];
        original.to_bytes(&mut buf);

        let loaded = MmapCache::from_bytes(&buf).expect("should parse");
        assert_eq!(loaded.index_mtime, original.index_mtime);
        assert_eq!(loaded.head_oid, original.head_oid);
        assert_eq!(loaded.files_changed, original.files_changed);
        assert_eq!(loaded.lines_added, original.lines_added);
        assert_eq!(loaded.lines_deleted, original.lines_deleted);
        assert_eq!(loaded.ahead, original.ahead);
        assert_eq!(loaded.behind, original.behind);
    }

    #[test]
    fn cache_invalid_magic() {
        let mut buf = [0u8; CACHE_SIZE];
        buf[0..4].copy_from_slice(b"XXXX"); // Wrong magic
        assert!(MmapCache::from_bytes(&buf).is_none());
    }

    #[test]
    fn cache_wrong_version() {
        let mut buf = [0u8; CACHE_SIZE];
        buf[0..4].copy_from_slice(CACHE_MAGIC);
        buf[4..8].copy_from_slice(&99u32.to_le_bytes()); // Wrong version
        assert!(MmapCache::from_bytes(&buf).is_none());
    }

    #[test]
    fn cache_truncated() {
        let buf = [0u8; 10]; // Too small
        assert!(MmapCache::from_bytes(&buf).is_none());
    }

    #[test]
    fn cache_head_oid_matches_prefix() {
        let cache = MmapCache {
            head_oid: *b"abc123def456abc123def456abc123def4567890",
            ..Default::default()
        };

        // Full match
        assert!(cache.head_oid_matches("abc123def456abc123def456abc123def4567890"));
        // Prefix match (short oid)
        assert!(cache.head_oid_matches("abc123"));
        assert!(cache.head_oid_matches("abc123def456"));
        // No match
        assert!(!cache.head_oid_matches("xyz"));
        assert!(!cache.head_oid_matches("abc124")); // Different character
    }

    #[test]
    fn cache_head_oid_empty_matches() {
        let cache = MmapCache::default();
        // Empty oid should match empty string
        assert!(cache.head_oid_matches(""));
    }

    // =========================================================================
    // write_tokens tests
    // =========================================================================

    #[test]
    fn tokens_small() {
        let mut buf = Vec::new();
        write_tokens(&mut buf, 42);
        assert_eq!(String::from_utf8(buf).unwrap(), "42");
    }

    #[test]
    fn tokens_thousands() {
        let mut buf = Vec::new();
        write_tokens(&mut buf, 5_432);
        assert_eq!(String::from_utf8(buf).unwrap(), "5K");
    }

    #[test]
    fn tokens_exact_thousand() {
        let mut buf = Vec::new();
        write_tokens(&mut buf, 1_000);
        assert_eq!(String::from_utf8(buf).unwrap(), "1K");
    }

    #[test]
    fn tokens_millions() {
        let mut buf = Vec::new();
        write_tokens(&mut buf, 2_500_000);
        assert_eq!(String::from_utf8(buf).unwrap(), "2.5M");
    }

    #[test]
    fn tokens_exact_million() {
        let mut buf = Vec::new();
        write_tokens(&mut buf, 1_000_000);
        assert_eq!(String::from_utf8(buf).unwrap(), "1.0M");
    }

    #[test]
    fn tokens_zero() {
        let mut buf = Vec::new();
        write_tokens(&mut buf, 0);
        assert_eq!(String::from_utf8(buf).unwrap(), "0");
    }

    #[test]
    fn tokens_large_millions() {
        let mut buf = Vec::new();
        write_tokens(&mut buf, 15_700_000);
        assert_eq!(String::from_utf8(buf).unwrap(), "15.7M");
    }

    // =========================================================================
    // get_worktree_name tests
    // =========================================================================

    #[test]
    fn worktree_name_linked() {
        let git_dir = "/home/user/project/.git/worktrees/feature-branch";
        let result = get_worktree_name(git_dir);
        assert_eq!(result, Some("feature-branch".to_string()));
    }

    #[test]
    fn worktree_name_linked_trailing_slash() {
        let git_dir = "/home/user/project/.git/worktrees/feature-branch/";
        let result = get_worktree_name(git_dir);
        assert_eq!(result, Some("feature-branch".to_string()));
    }

    #[test]
    fn worktree_name_main_repo() {
        // Main repo has git_dir like /path/.git, not a worktree
        let git_dir = "/home/user/project/.git";
        let result = get_worktree_name(git_dir);
        assert_eq!(result, None);
    }

    #[test]
    fn worktree_name_empty_name() {
        // Edge case: empty worktree name (shouldn't happen in practice)
        let git_dir = "/home/user/project/.git/worktrees/";
        let result = get_worktree_name(git_dir);
        assert_eq!(result, None);
    }

    #[test]
    fn worktree_name_nested_path() {
        // Worktree name with nested structure (rare but possible)
        let git_dir = "/repo/.git/worktrees/release-v1";
        let result = get_worktree_name(git_dir);
        assert_eq!(result, Some("release-v1".to_string()));
    }
}
