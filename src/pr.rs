//! Pull-request data: the on-disk cache codec, the pure response parser, and the
//! gh-CLI / native-HTTP refresh adapters.

use crate::git::GitRepo;
use crate::{atomic_rename, get_cache_dir, get_github_token, is_gh_available, unique_hex};
use cc_statusline::{hash_path, parse_github_url, percent_encode, shell_escape};
use serde::Deserialize;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::SystemTime;

/// PR cache data - parsed from gh JSON output
#[derive(Default, Clone)]
pub(crate) struct PrCacheData {
    pub(crate) number: u32,
    pub(crate) state: String,
    pub(crate) url: String,
    pub(crate) comments: u32,
    pub(crate) changed_files: u32,
    pub(crate) check_status: String, // "passed", "failed", "pending", ""
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

// ----------------------------------------------------------------------------
// PR cache codec — the single home for the on-disk format.
//
// Format: `timestamp\nbranch\npayload`, where payload is a JSON blob, the
// `NO_PR` negative-cache marker, or an `ERROR:...` marker. Every Rust writer
// goes through `encode_pr_cache`; the one exception is the detached gh refresh
// shell script (`spawn_pr_refresh_gh`), which builds the same layout via printf
// because it runs in a separate process.
// ponytail: keep this format dead simple (line-delimited); switch to a struct +
// serde only if a field ever needs escaping.
// ----------------------------------------------------------------------------

fn encode_pr_cache(timestamp: u64, branch: &str, payload: &str) -> String {
    format!("{timestamp}\n{branch}\n{payload}")
}

struct DecodedPrCache {
    timestamp: u64,
    branch: String,
    payload: String,
}

fn decode_pr_cache(content: &str) -> Option<DecodedPrCache> {
    let mut lines = content.lines();
    let timestamp = lines.next()?.parse().ok()?;
    let branch = lines.next()?.to_string();
    let payload = lines.collect::<Vec<_>>().join("\n");
    Some(DecodedPrCache {
        timestamp,
        branch,
        payload,
    })
}

/// Reduce a check-run rollup to "passed" / "failed" / "pending" / "".
///
/// gh CLI returns uppercase conclusions (`SUCCESS`), the REST API lowercase
/// (`success`); matched case-insensitively. Any non-passing conclusion is a
/// failure; a missing conclusion is pending.
fn compute_check_status(rollup: Option<&[GhCheckRun]>) -> String {
    let checks = match rollup {
        Some(c) if !c.is_empty() => c,
        _ => return String::new(),
    };

    let is_passing = |s: &str| {
        s.eq_ignore_ascii_case("SUCCESS")
            || s.eq_ignore_ascii_case("SKIPPED")
            || s.eq_ignore_ascii_case("NEUTRAL")
    };

    let has_failure = checks.iter().any(|c| match c.conclusion.as_deref() {
        Some(conc) if is_passing(conc) => false,
        Some(_) => true, // FAILURE, CANCELLED, TIMED_OUT, ACTION_REQUIRED, etc.
        None => false,
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

/// Parse a PR JSON payload (gh CLI or native format) into validated PR data.
/// Returns None when required fields are missing or invalid (caller: treat as stale).
fn parse_pr_payload(json_str: &str) -> Option<PrCacheData> {
    let pr: GhPrJson = serde_json::from_str(json_str).ok()?;
    let check_status = compute_check_status(pr.status_check_rollup.as_deref());

    #[allow(clippy::cast_possible_truncation)] // PR numbers/counts won't exceed u32::MAX
    let number = match pr.number {
        Some(n) if n > 0 => n as u32,
        _ => return None,
    };
    let state = match pr.state {
        Some(s) if !s.is_empty() => s,
        _ => return None,
    };
    let url = match pr.url {
        Some(u) if !u.is_empty() => u,
        _ => return None,
    };

    // Prefer commentsCount (numeric) over comments array to avoid large allocations
    #[allow(clippy::cast_possible_truncation)] // PR numbers/counts won't exceed u32::MAX
    let comments = pr
        .comments_count
        .map(|c| c as u32)
        .or_else(|| pr.comments.map(|c| c.len() as u32))
        .unwrap_or(0);

    #[allow(clippy::cast_possible_truncation)] // PR numbers/counts won't exceed u32::MAX
    Some(PrCacheData {
        number,
        state,
        url,
        comments,
        changed_files: pr.changed_files.unwrap_or(0) as u32,
        check_status,
    })
}

/// Load PR cache - reads file once and handles all states
fn load_pr_cache(repo_path: &str, branch: &str) -> PrCacheResult {
    let cache_path = get_pr_cache_path(repo_path, branch);
    let Ok(content) = fs::read_to_string(&cache_path) else {
        return PrCacheResult::Stale;
    };
    let Some(decoded) = decode_pr_cache(&content) else {
        return PrCacheResult::Stale;
    };

    // Validate branch matches
    if decoded.branch != branch {
        let _ = fs::remove_file(&cache_path);
        return PrCacheResult::Stale;
    }

    let now = SystemTime::now()
        .duration_since(SystemTime::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0);
    let age = now.saturating_sub(decoded.timestamp);
    let payload = decoded.payload;

    // NO_PR marker - negative cache with longer TTL
    if payload == "NO_PR" {
        if age < PR_NEGATIVE_CACHE_TTL {
            return PrCacheResult::NoPr;
        }
        return PrCacheResult::Stale;
    }

    // ERROR marker - don't cache errors, always retry
    if payload.starts_with("ERROR:") {
        return PrCacheResult::Stale;
    }

    // Normal TTL
    if age > PR_CACHE_TTL {
        return PrCacheResult::Stale;
    }

    match parse_pr_payload(&payload) {
        Some(data) => PrCacheResult::Hit(data),
        None => PrCacheResult::Stale,
    }
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
    // ponytail: this printf must mirror encode_pr_cache's `timestamp\nbranch\npayload`
    // layout by hand — it runs in a detached process and can't call back into Rust.
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
                encode_pr_cache(now, branch, "NO_PR")
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

                encode_pr_cache(now, branch, &gh_json.to_string())
            }
        }
        Err(ureq::Error::Status(code, _)) => {
            // API error (401/403/404 etc) - don't negative cache
            // Note: 404 can mean "no access" for private repos, not just "no PR"
            encode_pr_cache(now, branch, &format!("ERROR:HTTP {code}"))
        }
        Err(e) => {
            // Network error - don't negative cache
            encode_pr_cache(now, branch, &format!("ERROR:{e}"))
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
pub(crate) fn get_pr_data(git: &GitRepo) -> Option<PrCacheData> {
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pr_cache_round_trips() {
        let body = encode_pr_cache(1234567890, "feature/x", "{\"number\":7}");
        let d = decode_pr_cache(&body).expect("decodes");
        assert_eq!(d.timestamp, 1234567890);
        assert_eq!(d.branch, "feature/x");
        assert_eq!(d.payload, "{\"number\":7}");
    }

    #[test]
    fn pr_cache_decode_preserves_multiline_payload() {
        // Payload (pretty JSON) may contain newlines; decode must keep them all.
        let body = encode_pr_cache(1, "main", "{\n  \"a\": 1\n}");
        let d = decode_pr_cache(&body).expect("decodes");
        assert_eq!(d.payload, "{\n  \"a\": 1\n}");
    }

    #[test]
    fn pr_cache_decode_rejects_garbage() {
        assert!(decode_pr_cache("").is_none());
        assert!(decode_pr_cache("not-a-number\nmain\n{}").is_none());
        assert!(decode_pr_cache("123").is_none()); // missing branch line
    }

    #[test]
    fn check_status_empty_and_none() {
        assert_eq!(compute_check_status(None), "");
        assert_eq!(compute_check_status(Some(&[])), "");
    }

    fn run(conclusion: Option<&str>) -> GhCheckRun {
        GhCheckRun {
            conclusion: conclusion.map(String::from),
        }
    }

    #[test]
    fn check_status_case_insensitive_pass() {
        // gh CLI uppercase + REST API lowercase both count as passing.
        let runs = [
            run(Some("SUCCESS")),
            run(Some("skipped")),
            run(Some("NEUTRAL")),
        ];
        assert_eq!(compute_check_status(Some(&runs)), "passed");
    }

    #[test]
    fn check_status_any_failure_wins() {
        let runs = [run(Some("success")), run(Some("FAILURE")), run(None)];
        assert_eq!(compute_check_status(Some(&runs)), "failed");
    }

    #[test]
    fn check_status_pending_when_unfinished() {
        // A missing conclusion with no failures means still running.
        let runs = [run(Some("SUCCESS")), run(None)];
        assert_eq!(compute_check_status(Some(&runs)), "pending");
    }

    #[test]
    fn parse_pr_payload_native_format() {
        let json = r#"{"number":42,"state":"open","url":"https://x/42","commentsCount":3,"changedFiles":5,"statusCheckRollup":[{"conclusion":"SUCCESS"}]}"#;
        let pr = parse_pr_payload(json).expect("parses");
        assert_eq!(pr.number, 42);
        assert_eq!(pr.state, "open");
        assert_eq!(pr.comments, 3);
        assert_eq!(pr.changed_files, 5);
        assert_eq!(pr.check_status, "passed");
    }

    #[test]
    fn parse_pr_payload_counts_comments_array() {
        // gh CLI format: comments is an array, no commentsCount field.
        let json = r#"{"number":1,"state":"open","url":"u","comments":[{},{}]}"#;
        let pr = parse_pr_payload(json).expect("parses");
        assert_eq!(pr.comments, 2);
    }

    #[test]
    fn parse_pr_payload_rejects_incomplete() {
        assert!(parse_pr_payload("{}").is_none()); // no number
        assert!(parse_pr_payload(r#"{"number":0,"state":"open","url":"u"}"#).is_none());
        assert!(parse_pr_payload(r#"{"number":1,"state":"","url":"u"}"#).is_none());
        assert!(parse_pr_payload(r#"{"number":1,"state":"open","url":""}"#).is_none());
        assert!(parse_pr_payload("not json").is_none());
    }
}
