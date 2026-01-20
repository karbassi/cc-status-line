use git2::{DiffOptions, Repository};
use serde::Deserialize;
use std::borrow::Cow;
use std::env;
use std::io::{self, BufWriter, Read, Write};
use std::path::Path;
use std::sync::OnceLock;

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

/// Holds repository state for lazy evaluation of expensive git operations
struct GitRepo {
    repo: Repository,
    branch: String,
    worktree: Option<String>,
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
}

fn main() {
    let mut input = String::with_capacity(4096);
    io::stdin().read_to_string(&mut input).unwrap_or_default();

    let data: ClaudeInput = serde_json::from_str(&input).unwrap_or_default();

    let current_dir: Cow<str> = match data.workspace.current_dir.as_deref() {
        Some(dir) => Cow::Borrowed(dir),
        None => Cow::Owned(env::current_dir().unwrap().to_string_lossy().into_owned()),
    };

    let term_width: usize = env::var("CC_STATUS_WIDTH")
        .ok()
        .and_then(|s| s.parse().ok())
        .unwrap_or(50);

    let stdout = io::stdout();
    let mut out = BufWriter::new(stdout.lock());

    // Row 1: Location
    write_row1(&mut out, &data, &current_dir, term_width);

    // Row 2: Git (lazy evaluation for expensive operations)
    let git_repo = get_git_repo(&current_dir);
    write_row2(&mut out, git_repo.as_ref());

    // Row 3: Claude info
    write_row3(&mut out, &data);

    // Row 4: Session
    write_row4(&mut out, &data);

    out.flush().unwrap_or_default();
}

fn write_row1<W: Write>(out: &mut W, data: &ClaudeInput, current_dir: &str, term_width: usize) {
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

    let path_width = term_width.saturating_sub(project_name.len()).saturating_sub(3).max(10);
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

fn get_git_repo(dir: &str) -> Option<GitRepo> {
    let repo = Repository::discover(dir).ok()?;

    // Extract branch name and worktree info, then drop the borrow
    let (branch, worktree) = {
        let head = repo.head().ok()?;
        if !head.is_branch() {
            return None;
        }
        let branch = head.shorthand()?.to_owned();
        let worktree = if repo.is_worktree() {
            repo.path().parent()
                .and_then(|p| p.file_name())
                .map(|n| n.to_string_lossy().into_owned())
        } else {
            None
        };
        (branch, worktree)
    };

    Some(GitRepo { repo, branch, worktree })
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

    // Lazy: only compute diff stats now (expensive ~7-9ms)
    if let Some(diff) = git.diff_stats() {
        if diff.files_changed > 0 || diff.lines_added > 0 || diff.lines_deleted > 0 {
            write!(out, "{SEP}").unwrap_or_default();
            if diff.files_changed > 0 {
                write!(out, "{TN_GRAY}{} files{RESET}", diff.files_changed).unwrap_or_default();
            }
            if diff.lines_added > 0 {
                if diff.files_changed > 0 { write!(out, " ").unwrap_or_default(); }
                write!(out, "{TN_GREEN}+{}{RESET}", diff.lines_added).unwrap_or_default();
            }
            if diff.lines_deleted > 0 {
                if diff.files_changed > 0 || diff.lines_added > 0 { write!(out, " ").unwrap_or_default(); }
                write!(out, "{TN_RED}-{}{RESET}", diff.lines_deleted).unwrap_or_default();
            }
        }
    }

    // Lazy: only compute ahead/behind now (~1-2ms)
    if let Some(ab) = git.ahead_behind() {
        if ab.ahead > 0 || ab.behind > 0 {
            write!(out, "{SEP}").unwrap_or_default();
            if ab.ahead > 0 {
                write!(out, "{TN_GRAY}↑{}{RESET}", ab.ahead).unwrap_or_default();
            }
            if ab.behind > 0 {
                if ab.ahead > 0 { write!(out, " ").unwrap_or_default(); }
                write!(out, "{TN_GRAY}↓{}{RESET}", ab.behind).unwrap_or_default();
            }
        }
    }

    writeln!(out).unwrap_or_default();
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
