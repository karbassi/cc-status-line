use git2::{DiffOptions, Repository};
use memmap2::{MmapMut, MmapOptions};
use serde::Deserialize;
use std::borrow::Cow;
use std::env;
use std::fs::{self, OpenOptions};
use std::io::{self, BufWriter, Read, Write};
use std::path::Path;
use std::sync::OnceLock;
use std::time::SystemTime;

static HOME_DIR: OnceLock<String> = OnceLock::new();

fn get_home() -> &'static str {
    HOME_DIR.get_or_init(|| env::var("HOME").unwrap_or_default())
}

// Tokyo Night Dim Colors
const RESET: &str = "\x1b[0m";
const TN_BLUE: &str = "\x1b[2;38;2;122;162;247m";
const TN_CYAN: &str = "\x1b[2;38;2;125;207;255m";
const TN_PURPLE: &str = "\x1b[2;38;2;187;154;247m";
const TN_MAGENTA: &str = "\x1b[2;38;2;157;124;216m";
const TN_GREEN: &str = "\x1b[2;38;2;158;206;106m";
const TN_ORANGE: &str = "\x1b[2;38;2;255;158;100m";
const TN_TEAL: &str = "\x1b[2;38;2;42;195;222m";
const TN_GRAY: &str = "\x1b[2;38;2;86;95;137m";
const TN_RED: &str = "\x1b[2;38;2;247;118;142m";

const SEP: &str = "\x1b[2;38;2;86;95;137m • \x1b[0m";
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

struct DiffStats {
    files_changed: u32,
    lines_added: u32,
    lines_deleted: u32,
}

struct AheadBehind {
    ahead: u32,
    behind: u32,
}

/// Binary cache format for mmap (fixed 128 bytes)
/// Layout:
///   0-3:   magic "CCST"
///   4-7:   version (1)
///   8-15:  index_mtime (u64 LE)
///   16-55: head_oid (40 bytes, null-padded)
///   56-59: files_changed (u32 LE)
///   60-63: lines_added (u32 LE)
///   64-67: lines_deleted (u32 LE)
///   68-71: ahead (u32 LE)
///   72-75: behind (u32 LE)
///   76-127: reserved
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
        if data.len() < CACHE_SIZE {
            return None;
        }
        // Check magic
        if &data[0..4] != CACHE_MAGIC {
            return None;
        }
        // Check version
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

/// Holds repository state for lazy evaluation of expensive git operations
struct GitRepo {
    repo: Repository,
    branch: String,
    worktree: Option<String>,
    git_dir: String,
}

impl GitRepo {
    /// Compute diff stats lazily - this is the expensive operation (~7-9ms)
    fn diff_stats(&self) -> Option<DiffStats> {
        let head = self.repo.head().ok()?;
        let head_commit = head.peel_to_commit().ok()?;
        let head_tree = head_commit.tree().ok()?;

        let mut opts = DiffOptions::new();
        opts.include_untracked(false);

        let diff = self.repo.diff_tree_to_workdir_with_index(Some(&head_tree), Some(&mut opts)).ok()?;
        let stats = diff.stats().ok()?;

        Some(DiffStats {
            files_changed: stats.files_changed() as u32,
            lines_added: stats.insertions() as u32,
            lines_deleted: stats.deletions() as u32,
        })
    }

    /// Compute ahead/behind lazily (~1-2ms)
    fn ahead_behind(&self) -> Option<AheadBehind> {
        let head = self.repo.head().ok()?;
        let local_oid = head.target()?;
        let branch = self.repo.find_branch(&self.branch, git2::BranchType::Local).ok()?;
        let upstream = branch.upstream().ok()?;
        let upstream_oid = upstream.get().target()?;
        let (ahead, behind) = self.repo.graph_ahead_behind(local_oid, upstream_oid).ok()?;

        Some(AheadBehind {
            ahead: ahead as u32,
            behind: behind as u32,
        })
    }

    /// Get index mtime for cache invalidation
    fn index_mtime(&self) -> u64 {
        let index_path = format!("{}/index", self.git_dir);
        fs::metadata(&index_path)
            .and_then(|m| m.modified())
            .map(|t| t.duration_since(SystemTime::UNIX_EPOCH).unwrap_or_default().as_secs())
            .unwrap_or(0)
    }

    /// Get HEAD oid for cache invalidation (reads file directly, avoids repo.head())
    fn head_oid(&self) -> String {
        // Try to read HEAD oid directly from refs file (much faster than repo.head())
        let ref_path = format!("{}refs/heads/{}", self.git_dir, self.branch);
        if let Ok(oid) = fs::read_to_string(&ref_path) {
            return oid.trim().to_string();
        }
        // Fallback to packed-refs or repo.head() if ref file doesn't exist
        self.repo.head()
            .ok()
            .and_then(|h| h.target())
            .map(|oid| oid.to_string())
            .unwrap_or_default()
    }
}

fn get_cache_path(git_dir: &str) -> String {
    format!("/tmp/cc-status-{:016x}.cache", hash_path(git_dir))
}

/// Try to load cached git state via mmap
fn load_mmap_cache(git_dir: &str) -> Option<MmapCache> {
    let cache_path = get_cache_path(git_dir);
    let file = OpenOptions::new().read(true).open(&cache_path).ok()?;

    // Safety: we only read, and handle invalid data gracefully
    let mmap = unsafe { MmapOptions::new().map(&file).ok()? };

    MmapCache::from_bytes(&mmap)
}

/// Save git state to cache via mmap
fn save_mmap_cache(git_dir: &str, cache: &MmapCache) {
    let cache_path = get_cache_path(git_dir);

    // Create or truncate file
    let file = match OpenOptions::new()
        .read(true)
        .write(true)
        .create(true)
        .truncate(true)
        .open(&cache_path)
    {
        Ok(f) => f,
        Err(_) => return,
    };

    // Set file size
    if file.set_len(CACHE_SIZE as u64).is_err() {
        return;
    }

    // Map and write
    // Safety: we control the file exclusively during write
    let mut mmap = match unsafe { MmapMut::map_mut(&file) } {
        Ok(m) => m,
        Err(_) => return,
    };

    cache.to_bytes(&mut mmap);
    let _ = mmap.flush();
}

/// Cached git repo info (path + branch)
struct GitPathCache {
    git_path: String,
    branch: String,
}

fn get_head_mtime(git_path: &str) -> u64 {
    let head_path = format!("{}HEAD", git_path);
    fs::metadata(&head_path)
        .and_then(|m| m.modified())
        .map(|t| t.duration_since(SystemTime::UNIX_EPOCH).unwrap_or_default().as_secs())
        .unwrap_or(0)
}

/// Get cached git info for a working directory
fn get_cached_git_info(working_dir: &str) -> Option<GitPathCache> {
    let cache_path = format!("/tmp/cc-gitpath-{:016x}.cache", hash_path(working_dir));

    let content = fs::read_to_string(&cache_path).ok()?;
    let mut lines = content.lines();

    let git_path = lines.next()?.to_string();
    let branch = lines.next()?.to_string();
    let cached_mtime: u64 = lines.next()?.parse().ok()?;

    // Verify git path exists
    if !Path::new(&git_path).exists() {
        let _ = fs::remove_file(&cache_path);
        return None;
    }

    // Check if HEAD has changed (branch switch, commit, etc.)
    let current_mtime = get_head_mtime(&git_path);
    if current_mtime != cached_mtime {
        return None; // Cache invalid, need to re-read branch
    }

    Some(GitPathCache { git_path, branch })
}

/// Cache git info for a working directory
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

    // Find segment boundaries (positions after each '/')
    let bytes = path.as_bytes();
    let mut seg_starts: [usize; 32] = [0; 32]; // Stack-allocated, supports up to 32 segments
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

    // Calculate lengths of last two segments
    let last_start = seg_starts[seg_count - 1];
    let parent_start = seg_starts[seg_count - 2];
    let last_seg = &path[last_start..];
    let parent_seg = &path[parent_start..last_start.saturating_sub(1)];

    // Try keeping parent intact: a/b/.../parent/last
    let abbrev_prefix_len = (seg_count - 2) * 2; // Each abbreviated segment = 1 char + '/'
    let try1_len = abbrev_prefix_len + parent_seg.len() + 1 + last_seg.len();

    let mut result = String::with_capacity(max_width + 10);

    if try1_len <= max_width || seg_count <= 2 {
        // Abbreviate all but last two segments
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
        // Abbreviate all but last segment
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

fn get_worktree_name(repo: &Repository) -> Option<String> {
    if repo.is_worktree() {
        repo.path().parent()
            .and_then(|p| p.file_name())
            .map(|n| n.to_string_lossy().into_owned())
    } else {
        None
    }
}

fn get_git_repo(dir: &str) -> Option<GitRepo> {
    // Try full cache (git_path + branch) first
    if let Some(cache) = get_cached_git_info(dir) {
        let repo = Repository::open(&cache.git_path).ok()?;
        let worktree = get_worktree_name(&repo);
        return Some(GitRepo {
            repo,
            branch: cache.branch,
            worktree,
            git_dir: cache.git_path,
        });
    }

    // No cache or invalid - do full discovery
    let repo = Repository::discover(dir).ok()?;
    let git_dir = repo.path().to_string_lossy().into_owned();

    let branch = {
        let head = repo.head().ok()?;
        if !head.is_branch() {
            return None;
        }
        head.shorthand()?.to_owned()
    };
    let worktree = get_worktree_name(&repo);

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
                // Cache hit - use mmap'd values directly
                (c.files_changed, c.lines_added, c.lines_deleted, c.ahead, c.behind)
            } else {
                // Cache miss - compute fresh
                compute_and_cache_git_stats(git, current_mtime, &current_oid)
            }
        } else {
            // No cache - compute fresh
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

fn compute_and_cache_git_stats(git: &GitRepo, mtime: u64, oid: &str) -> (u32, u32, u32, u32, u32) {
    let diff = git.diff_stats();
    let ab = git.ahead_behind();

    let files_changed = diff.as_ref().map(|d| d.files_changed).unwrap_or(0);
    let lines_added = diff.as_ref().map(|d| d.lines_added).unwrap_or(0);
    let lines_deleted = diff.as_ref().map(|d| d.lines_deleted).unwrap_or(0);
    let ahead = ab.as_ref().map(|a| a.ahead).unwrap_or(0);
    let behind = ab.as_ref().map(|a| a.behind).unwrap_or(0);

    // Save to mmap cache
    let mut cache = MmapCache::default();
    cache.index_mtime = mtime;
    // Copy OID bytes (null-padded)
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
        // Use integer math: n / 100_000 gives us tenths of millions
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
