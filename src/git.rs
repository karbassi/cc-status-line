//! Git repository state and the mmap-backed diff-stat cache.

use crate::{atomic_rename, get_cache_dir, unique_hex};
use cc_statusline::hash_path;
use gix::Repository;
use memmap2::{MmapMut, MmapOptions};
use std::fs::{self, OpenOptions};
use std::path::{Path, PathBuf};
use std::time::SystemTime;

/// Binary cache format for mmap (fixed 128 bytes)
const CACHE_SIZE: usize = 128;
const CACHE_MAGIC: &[u8; 4] = b"CCST";
const CACHE_VERSION: u32 = 1;

pub(crate) struct MmapCache {
    pub(crate) index_mtime: u64,
    pub(crate) head_oid: [u8; 40],
    pub(crate) files_changed: u32,
    pub(crate) lines_added: u32,
    pub(crate) lines_deleted: u32,
    pub(crate) ahead: u32,
    pub(crate) behind: u32,
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

    pub(crate) fn head_oid_matches(&self, oid: &str) -> bool {
        let oid_bytes = oid.as_bytes();
        oid_bytes.len() <= 40 && self.head_oid[..oid_bytes.len()] == *oid_bytes
    }
}

/// Holds repository state for lazy evaluation of expensive git operations
pub(crate) struct GitRepo {
    pub(crate) repo: Repository,
    pub(crate) branch: String,
    pub(crate) worktree: Option<String>,
    pub(crate) git_dir: String,
    pub(crate) work_dir: String,
}

impl GitRepo {
    /// Count tracked files that differ between the index and the working tree.
    ///
    /// Uses gix's content-aware status (the same racy-clean handling git does), so a
    /// file whose mtime drifted but whose content is unchanged is NOT counted — the
    /// old mtime-only heuristic reported those as false positives. Untracked files are
    /// excluded (dirwalk disabled), matching the previous index-only semantics; like
    /// git's own dirty check this compares worktree-vs-index, not index-vs-HEAD, so
    /// staged-only changes aren't counted. Line counts aren't computed.
    fn diff_stats(&self) -> Option<(u32, u32, u32)> {
        let files = self
            .repo
            .status(gix::progress::Discard)
            .ok()?
            .index_worktree_rewrites(None)
            .index_worktree_submodules(gix::status::Submodule::AsConfigured { check_dirty: true })
            .index_worktree_options_mut(|opts| {
                opts.dirwalk_options = None;
            })
            .into_index_worktree_iter(Vec::new())
            .ok()?
            // Count every yielded item, including per-path `Err`s: for a status line an
            // errored/unreadable path is "unknown, treat as dirty", never silently dropped.
            .count() as u32;

        Some((files, 0, 0))
    }

    /// Get index mtime for cache invalidation
    pub(crate) fn index_mtime(&self) -> u64 {
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
    pub(crate) fn head_oid(&self) -> String {
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

pub(crate) fn load_mmap_cache(git_dir: &str) -> Option<MmapCache> {
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

pub(crate) fn get_git_repo(dir: &str) -> Option<GitRepo> {
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
pub(crate) fn get_ahead_behind(repo: &gix::Repository, branch: &str) -> (u32, u32) {
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

pub(crate) fn compute_and_cache_git_stats(git: &GitRepo, mtime: u64, oid: &str) -> (u32, u32, u32) {
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

#[cfg(test)]
mod tests {
    use super::*;

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

    #[test]
    fn worktree_name_linked() {
        let git_dir = "/home/user/project/.git/worktrees/feature-branch";
        assert_eq!(
            get_worktree_name(git_dir),
            Some("feature-branch".to_string())
        );
    }

    #[test]
    fn worktree_name_linked_trailing_slash() {
        let git_dir = "/home/user/project/.git/worktrees/feature-branch/";
        assert_eq!(
            get_worktree_name(git_dir),
            Some("feature-branch".to_string())
        );
    }

    #[test]
    fn worktree_name_main_repo() {
        // Main repo has git_dir like /path/.git, not a worktree
        let git_dir = "/home/user/project/.git";
        assert_eq!(get_worktree_name(git_dir), None);
    }

    #[test]
    fn worktree_name_empty_name() {
        // Edge case: empty worktree name (shouldn't happen in practice)
        let git_dir = "/home/user/project/.git/worktrees/";
        assert_eq!(get_worktree_name(git_dir), None);
    }

    #[test]
    fn worktree_name_nested_path() {
        // Worktree name with nested structure (rare but possible)
        let git_dir = "/repo/.git/worktrees/release-v1";
        assert_eq!(get_worktree_name(git_dir), Some("release-v1".to_string()));
    }
}
