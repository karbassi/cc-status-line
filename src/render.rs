//! Config-driven rendering. `gather` resolves a `StatusView` (all I/O lives here);
//! `render_component` / `write_rows` are pure over it — the test surface.

use crate::git::{GitRepo, compute_and_cache_git_stats, get_ahead_behind, load_mmap_cache};
use crate::pr::{PrCacheData, get_pr_data};
use crate::{ClaudeInput, Config, get_home, get_hostname, is_ssh_session};
use cc_statusline::abbreviate_path;
use std::io::Write;
use std::path::Path;

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

fn format_tokens(n: u64) -> String {
    if n >= 1_000_000 {
        let tenths = n / 100_000;
        let whole = tenths / 10;
        let frac = tenths % 10;
        format!("{whole}.{frac}M")
    } else if n >= 1_000 {
        format!("{}K", n / 1_000)
    } else {
        format!("{n}")
    }
}

/// Plain, fully-resolved data the status line renders from.
///
/// This is the seam: `gather` does all I/O (git, PR fetch, hostname) and produces
/// a `StatusView`; `render_component` consumes one and touches nothing else. Tests
/// build a `StatusView` literal and assert exact output — no process spawn, no git,
/// no network.
pub(crate) struct StatusView {
    hostname: Option<String>,
    project_name: String,
    display_cwd: String,
    branch: Option<String>,
    worktree: Option<String>,
    files_changed: u32,
    ahead: u32,
    behind: u32,
    pr: Option<PrCacheData>,
    model: Option<String>,
    context_pct: Option<f64>,
    output_style: Option<String>,
    duration_ms: u64,
    input_tokens: u64,
    output_tokens: u64,
}

/// Resolve a `StatusView` from input and the discovered repo, running all I/O here.
pub(crate) fn gather(data: &ClaudeInput, current_dir: &str, git: Option<&GitRepo>) -> StatusView {
    let project_name = data
        .workspace
        .project_dir
        .as_ref()
        .and_then(|p| Path::new(p).file_name())
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_default();

    let home = get_home();
    let display_cwd = if !home.is_empty() && current_dir.starts_with(home) {
        format!("~{}", &current_dir[home.len()..])
    } else {
        current_dir.to_string()
    };

    let hostname = if is_ssh_session() {
        get_hostname().cloned()
    } else {
        None
    };

    // Compute git stats upfront if we have a git repo and no JSON override
    let (files_changed, ahead, behind) = if data.git.branch.is_some() {
        // Using JSON input
        (
            data.git.changed_files.unwrap_or(0),
            data.git.ahead.unwrap_or(0),
            data.git.behind.unwrap_or(0),
        )
    } else if let Some(g) = git {
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

        let (ahead, behind) = get_ahead_behind(&g.repo, &g.branch);
        (files, ahead, behind)
    } else {
        (0, 0, 0)
    };

    // Get PR data
    let pr = if data.pr.number.is_some() {
        // Using JSON input
        Some(PrCacheData {
            number: data.pr.number.unwrap_or(0),
            state: data.pr.state.clone().unwrap_or_default(),
            url: data.pr.url.clone().unwrap_or_default(),
            comments: data.pr.comments.unwrap_or(0),
            changed_files: data.pr.changed_files.unwrap_or(0),
            check_status: data.pr.check_status.clone().unwrap_or_default(),
        })
    } else {
        git.and_then(get_pr_data)
    };

    let branch = data
        .git
        .branch
        .clone()
        .or_else(|| git.map(|g| g.branch.clone()));
    let worktree = data
        .git
        .worktree
        .clone()
        .or_else(|| git.and_then(|g| g.worktree.clone()));

    StatusView {
        hostname,
        project_name,
        display_cwd,
        branch,
        worktree,
        files_changed,
        ahead,
        behind,
        pr,
        model: data.model.display_name.clone(),
        context_pct: data.context_window.remaining_percentage,
        output_style: data.output_style.name.clone(),
        duration_ms: data.cost.total_duration_ms.unwrap_or(0),
        input_tokens: data.context_window.total_input_tokens.unwrap_or(0),
        output_tokens: data.context_window.total_output_tokens.unwrap_or(0),
    }
}

/// Render a single component, returning colored output string or None if no data
fn render_component(name: &str, view: &StatusView) -> Option<String> {
    match name {
        "hostname" => view
            .hostname
            .as_ref()
            .map(|h| format!("{TN_GREEN}{h}{RESET}")),

        "project" => {
            if view.project_name.is_empty() {
                None
            } else {
                Some(format!("{TN_BLUE}{}{RESET}", view.project_name))
            }
        }

        "path" => {
            // Use a conservative width for path abbreviation
            // Since config allows placing path on any row, we can't know what other
            // components share the row. Use ~60% of terminal width as a reasonable default.
            let path_width = (TERM_WIDTH * 3 / 5).max(20);
            let abbrev = abbreviate_path(&view.display_cwd, path_width);
            Some(format!("{TN_CYAN}{abbrev}{RESET}"))
        }

        "branch" => view
            .branch
            .as_deref()
            .map(|b| format!("{TN_PURPLE}{b}{RESET}")),

        // Shows "no git" when there's no branch (not in a git repo)
        "no_git" => {
            if view.branch.is_none() {
                Some(format!("{TN_GRAY}no git{RESET}"))
            } else {
                None
            }
        }

        "worktree" => view
            .worktree
            .as_deref()
            .map(|wt| format!("{TN_MAGENTA}{wt}{RESET}")),

        "files" => {
            let files = view.files_changed;
            if files > 0 {
                Some(format!("{TN_GRAY}{files} files{RESET}"))
            } else {
                None
            }
        }

        "ahead_behind" => {
            let (ahead, behind) = (view.ahead, view.behind);
            if ahead > 0 || behind > 0 {
                let mut s = String::new();
                if ahead > 0 {
                    s.push_str(&format!("{TN_GRAY}↑{ahead}{RESET}"));
                }
                if behind > 0 {
                    if ahead > 0 {
                        s.push(' ');
                    }
                    s.push_str(&format!("{TN_GRAY}↓{behind}{RESET}"));
                }
                Some(s)
            } else {
                None
            }
        }

        "pr_number" => {
            let pr = view.pr.as_ref()?;
            if pr.url.is_empty() {
                Some(format!("{TN_CYAN}#{}{RESET}", pr.number))
            } else {
                Some(format!(
                    "{OSC8_START}{}{OSC8_MID}{TN_CYAN}#{}{RESET}{OSC8_END}",
                    pr.url, pr.number
                ))
            }
        }

        "pr_state" => {
            let pr = view.pr.as_ref()?;
            let state_lower = pr.state.to_lowercase();
            let color = match state_lower.as_str() {
                "open" => TN_GREEN,
                "merged" => TN_PURPLE,
                "closed" => TN_RED,
                _ => TN_GRAY,
            };
            Some(format!("{color}{state_lower}{RESET}"))
        }

        "pr_comments" => {
            let pr = view.pr.as_ref()?;
            if pr.comments > 0 {
                let label = if pr.comments == 1 {
                    "comment"
                } else {
                    "comments"
                };
                Some(format!("{TN_GRAY}{} {label}{RESET}", pr.comments))
            } else {
                None
            }
        }

        "pr_files" => {
            let pr = view.pr.as_ref()?;
            if pr.changed_files > 0 {
                let label = if pr.changed_files == 1 {
                    "file"
                } else {
                    "files"
                };
                Some(format!("{TN_GRAY}{} {label}{RESET}", pr.changed_files))
            } else {
                None
            }
        }

        "pr_checks" => {
            let pr = view.pr.as_ref()?;
            let checks_url = if pr.url.is_empty() {
                String::new()
            } else {
                format!("{}/checks", pr.url)
            };
            match pr.check_status.trim() {
                "passed" if !checks_url.is_empty() => Some(format!(
                    "{OSC8_START}{checks_url}{OSC8_MID}{TN_GREEN}checks passed{RESET}{OSC8_END}"
                )),
                "failed" if !checks_url.is_empty() => Some(format!(
                    "{OSC8_START}{checks_url}{OSC8_MID}{TN_RED}checks failed{RESET}{OSC8_END}"
                )),
                "pending" if !checks_url.is_empty() => Some(format!(
                    "{OSC8_START}{checks_url}{OSC8_MID}{TN_ORANGE}checks pending{RESET}{OSC8_END}"
                )),
                "passed" => Some(format!("{TN_GREEN}checks passed{RESET}")),
                "failed" => Some(format!("{TN_RED}checks failed{RESET}")),
                "pending" => Some(format!("{TN_ORANGE}checks pending{RESET}")),
                _ => None,
            }
        }

        "model" => {
            if let Some(model) = &view.model
                && model != "Unknown"
            {
                return Some(format!("{TN_ORANGE}{model}{RESET}"));
            }
            None
        }

        "context" => {
            #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
            let pct = view.context_pct.unwrap_or(100.0) as u32;
            if pct < 100 {
                Some(format!("{TN_TEAL}{pct}%{RESET}"))
            } else {
                None
            }
        }

        "style" => {
            if let Some(mode) = &view.output_style
                && mode != "default"
            {
                return Some(format!("{TN_BLUE}{mode}{RESET}"));
            }
            None
        }

        "duration" => {
            let ms = view.duration_ms;
            if ms > 0 {
                let total_secs = ms / 1000;
                let mins = total_secs / 60;
                let hours = mins / 60;
                let mins = mins % 60;
                if hours > 0 {
                    Some(format!("{TN_GRAY}{hours}h {mins}m{RESET}"))
                } else {
                    Some(format!("{TN_GRAY}{mins}m{RESET}"))
                }
            } else {
                None
            }
        }

        "tokens" => {
            let input = view.input_tokens;
            let output = view.output_tokens;
            if input > 0 || output > 0 {
                Some(format!(
                    "{TN_GRAY}{}/{}{RESET}",
                    format_tokens(input),
                    format_tokens(output)
                ))
            } else {
                None
            }
        }

        _ => None, // Unknown component - ignore silently for forward compatibility
    }
}

/// Write all rows according to config
pub(crate) fn write_rows<W: Write>(out: &mut W, config: &Config, view: &StatusView) {
    for row_components in &config.rows {
        if row_components.is_empty() {
            continue;
        }

        let parts: Vec<String> = row_components
            .iter()
            .filter_map(|name| render_component(name, view))
            .collect();

        if !parts.is_empty() {
            writeln!(out, "{}", parts.join(SEP)).unwrap_or_default();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn empty_view() -> StatusView {
        StatusView {
            hostname: None,
            project_name: String::new(),
            display_cwd: String::new(),
            branch: None,
            worktree: None,
            files_changed: 0,
            ahead: 0,
            behind: 0,
            pr: None,
            model: None,
            context_pct: None,
            output_style: None,
            duration_ms: 0,
            input_tokens: 0,
            output_tokens: 0,
        }
    }

    #[test]
    fn tokens_small() {
        assert_eq!(format_tokens(42), "42");
    }

    #[test]
    fn tokens_thousands() {
        assert_eq!(format_tokens(5_432), "5K");
    }

    #[test]
    fn tokens_exact_thousand() {
        assert_eq!(format_tokens(1_000), "1K");
    }

    #[test]
    fn tokens_millions() {
        assert_eq!(format_tokens(2_500_000), "2.5M");
    }

    #[test]
    fn tokens_exact_million() {
        assert_eq!(format_tokens(1_000_000), "1.0M");
    }

    #[test]
    fn tokens_zero() {
        assert_eq!(format_tokens(0), "0");
    }

    #[test]
    fn tokens_large_millions() {
        assert_eq!(format_tokens(15_700_000), "15.7M");
    }

    #[test]
    fn render_model_skips_unknown() {
        let mut v = empty_view();
        v.model = Some("Unknown".to_string());
        assert_eq!(render_component("model", &v), None);

        v.model = Some("Opus".to_string());
        assert_eq!(
            render_component("model", &v),
            Some(format!("{TN_ORANGE}Opus{RESET}"))
        );
    }

    #[test]
    fn render_no_git_tracks_branch() {
        let v = empty_view();
        assert_eq!(
            render_component("no_git", &v),
            Some(format!("{TN_GRAY}no git{RESET}"))
        );

        let mut v2 = empty_view();
        v2.branch = Some("main".to_string());
        assert_eq!(render_component("no_git", &v2), None);
        assert_eq!(
            render_component("branch", &v2),
            Some(format!("{TN_PURPLE}main{RESET}"))
        );
    }

    #[test]
    fn render_tokens_formats_both() {
        let mut v = empty_view();
        assert_eq!(render_component("tokens", &v), None);

        v.input_tokens = 5_000;
        v.output_tokens = 1_500_000;
        assert_eq!(
            render_component("tokens", &v),
            Some(format!("{TN_GRAY}5K/1.5M{RESET}"))
        );
    }

    #[test]
    fn render_context_hides_at_full() {
        let mut v = empty_view();
        v.context_pct = Some(100.0);
        assert_eq!(render_component("context", &v), None);

        v.context_pct = Some(42.0);
        assert_eq!(
            render_component("context", &v),
            Some(format!("{TN_TEAL}42%{RESET}"))
        );
    }

    #[test]
    fn write_rows_skips_empty_rows() {
        let config = Config {
            rows: vec![vec!["model".to_string()], vec!["branch".to_string()]],
        };
        let mut v = empty_view();
        v.model = Some("Opus".to_string());

        let mut buf = Vec::new();
        write_rows(&mut buf, &config, &v);
        let out = String::from_utf8(buf).unwrap();

        // model row renders; branch row is empty and is skipped entirely
        assert_eq!(out.lines().count(), 1);
        assert!(out.contains("Opus"));
    }
}
