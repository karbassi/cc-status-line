use git2::{DiffOptions, Repository};
use serde::Deserialize;
use std::env;
use std::io::{self, BufWriter, Read, Write};
use std::path::Path;

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

#[derive(Default)]
struct GitInfo {
    branch: Option<String>,
    worktree: Option<String>,
    files_changed: u32,
    lines_added: u32,
    lines_deleted: u32,
    ahead: u32,
    behind: u32,
}

fn main() {
    let mut input = String::with_capacity(4096);
    io::stdin().read_to_string(&mut input).unwrap_or_default();

    let data: ClaudeInput = serde_json::from_str(&input).unwrap_or_default();

    let current_dir = data
        .workspace
        .current_dir
        .as_deref()
        .map(String::from)
        .unwrap_or_else(|| env::current_dir().unwrap().to_string_lossy().to_string());

    let term_width: usize = env::var("CC_STATUS_WIDTH")
        .ok()
        .and_then(|s| s.parse().ok())
        .unwrap_or(50);

    let stdout = io::stdout();
    let mut out = BufWriter::new(stdout.lock());

    // Row 1: Location
    write_row1(&mut out, &data, &current_dir, term_width);

    // Row 2: Git
    let git_info = get_git_info(&current_dir);
    write_row2(&mut out, &git_info);

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

    let home = env::var("HOME").unwrap_or_default();
    let display_cwd = if !home.is_empty() && current_dir.starts_with(&home) {
        format!("~{}", &current_dir[home.len()..])
    } else {
        current_dir.to_string()
    };

    let path_width = term_width.saturating_sub(project_name.len()).saturating_sub(3).max(10);
    let abbrev_cwd = abbreviate_path(&display_cwd, path_width);

    writeln!(out, "{TN_BLUE}{project_name}{RESET}{SEP}{TN_CYAN}{abbrev_cwd}{RESET}").unwrap_or_default();
}

fn abbreviate_path(path: &str, max_width: usize) -> String {
    if path.len() <= max_width {
        return path.to_string();
    }

    let segments: Vec<&str> = path.split('/').collect();
    let count = segments.len();

    if count < 2 {
        return path.to_string();
    }

    let mut result = String::with_capacity(max_width + 10);
    for seg in &segments[..count.saturating_sub(2)] {
        if let Some(c) = seg.chars().next() {
            result.push(c);
            result.push('/');
        }
    }
    if count >= 2 {
        result.push_str(segments[count - 2]);
        result.push('/');
    }
    result.push_str(segments[count - 1]);

    if result.len() > max_width && count > 2 {
        result.clear();
        for seg in &segments[..count - 1] {
            if let Some(c) = seg.chars().next() {
                result.push(c);
                result.push('/');
            }
        }
        result.push_str(segments[count - 1]);
    }

    result
}

fn get_git_info(dir: &str) -> GitInfo {
    let mut info = GitInfo::default();

    let repo = match Repository::discover(dir) {
        Ok(r) => r,
        Err(_) => return info,
    };

    // Get branch name
    if let Ok(head) = repo.head() {
        if head.is_branch() {
            info.branch = head.shorthand().map(String::from);
        }
    }

    if info.branch.is_none() {
        return info;
    }

    // Check for worktree
    if repo.is_worktree() {
        if let Some(name) = repo.path().parent().and_then(|p| p.file_name()) {
            info.worktree = Some(name.to_string_lossy().to_string());
        }
    }

    // Get diff stats (staged + unstaged vs HEAD)
    if let Ok(head_commit) = repo.head().and_then(|h| h.peel_to_commit()) {
        if let Ok(head_tree) = head_commit.tree() {
            let mut opts = DiffOptions::new();
            opts.include_untracked(false);

            if let Ok(diff) = repo.diff_tree_to_workdir_with_index(Some(&head_tree), Some(&mut opts)) {
                if let Ok(stats) = diff.stats() {
                    info.files_changed = stats.files_changed() as u32;
                    info.lines_added = stats.insertions() as u32;
                    info.lines_deleted = stats.deletions() as u32;
                }
            }
        }
    }

    // Get ahead/behind
    if let Ok(head) = repo.head() {
        if let Some(local_oid) = head.target() {
            // Try to find upstream branch
            if let Ok(branch) = repo.find_branch(
                head.shorthand().unwrap_or(""),
                git2::BranchType::Local,
            ) {
                if let Ok(upstream) = branch.upstream() {
                    if let Some(upstream_oid) = upstream.get().target() {
                        if let Ok((ahead, behind)) = repo.graph_ahead_behind(local_oid, upstream_oid) {
                            info.ahead = ahead as u32;
                            info.behind = behind as u32;
                        }
                    }
                }
            }
        }
    }

    info
}

fn write_row2<W: Write>(out: &mut W, git: &GitInfo) {
    match &git.branch {
        None => writeln!(out, "{TN_GRAY}no git{RESET}").unwrap_or_default(),
        Some(branch) => {
            write!(out, "{TN_PURPLE}{branch}{RESET}").unwrap_or_default();

            if let Some(wt) = &git.worktree {
                write!(out, "{SEP}{TN_MAGENTA}{wt}{RESET}").unwrap_or_default();
            }

            if git.files_changed > 0 || git.lines_added > 0 || git.lines_deleted > 0 {
                write!(out, "{SEP}").unwrap_or_default();
                if git.files_changed > 0 {
                    write!(out, "{TN_GRAY}{} files{RESET}", git.files_changed).unwrap_or_default();
                }
                if git.lines_added > 0 {
                    if git.files_changed > 0 { write!(out, " ").unwrap_or_default(); }
                    write!(out, "{TN_GREEN}+{}{RESET}", git.lines_added).unwrap_or_default();
                }
                if git.lines_deleted > 0 {
                    if git.files_changed > 0 || git.lines_added > 0 { write!(out, " ").unwrap_or_default(); }
                    write!(out, "{TN_RED}-{}{RESET}", git.lines_deleted).unwrap_or_default();
                }
            }

            if git.ahead > 0 || git.behind > 0 {
                write!(out, "{SEP}").unwrap_or_default();
                if git.ahead > 0 {
                    write!(out, "{TN_GRAY}↑{}{RESET}", git.ahead).unwrap_or_default();
                }
                if git.behind > 0 {
                    if git.ahead > 0 { write!(out, " ").unwrap_or_default(); }
                    write!(out, "{TN_GRAY}↓{}{RESET}", git.behind).unwrap_or_default();
                }
            }

            writeln!(out).unwrap_or_default();
        }
    }
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
        write!(out, "{TN_GRAY}{}/{}{RESET}", format_tokens(input_tokens), format_tokens(output_tokens)).unwrap_or_default();
        has_content = true;
    }

    if has_content {
        writeln!(out).unwrap_or_default();
    }
}

fn format_tokens(n: u64) -> String {
    if n >= 1_000_000 {
        format!("{:.1}M", n as f64 / 1_000_000.0)
    } else if n >= 1_000 {
        format!("{}K", n / 1_000)
    } else {
        n.to_string()
    }
}
