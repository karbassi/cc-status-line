use serde::{Deserialize, Serialize};
use std::borrow::Cow;
use std::env;
use std::fs;
use std::io::{self, BufWriter, Read, Write};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::OnceLock;

mod git;
mod pr;
mod render;

use git::get_git_repo;
use render::{gather, write_rows};

static HOME_DIR: OnceLock<String> = OnceLock::new();
static CACHE_DIR: OnceLock<PathBuf> = OnceLock::new();
static GH_AVAILABLE: OnceLock<bool> = OnceLock::new();
static HOSTNAME: OnceLock<Option<String>> = OnceLock::new();
static CONFIG: OnceLock<Config> = OnceLock::new();

/// Configuration for display customization
#[derive(Debug, Clone, Serialize, Deserialize)]
pub(crate) struct Config {
    /// Each inner Vec is one row, containing component names in display order
    pub(crate) rows: Vec<Vec<String>>,
}

impl Default for Config {
    fn default() -> Self {
        default_config()
    }
}

pub(crate) fn get_home() -> &'static str {
    HOME_DIR.get_or_init(|| {
        // Try HOME first (Unix standard), then USERPROFILE (Windows standard)
        env::var("HOME")
            .or_else(|_| env::var("USERPROFILE"))
            .unwrap_or_default()
    })
}

/// Get the default configuration (matches current hardcoded behavior)
fn default_config() -> Config {
    Config {
        rows: vec![
            vec![
                "hostname".to_string(),
                "project".to_string(),
                "path".to_string(),
            ],
            vec![
                "no_git".to_string(),
                "branch".to_string(),
                "worktree".to_string(),
                "files".to_string(),
                "ahead_behind".to_string(),
            ],
            vec![
                "pr_number".to_string(),
                "pr_state".to_string(),
                "pr_comments".to_string(),
                "pr_files".to_string(),
                "pr_checks".to_string(),
            ],
            vec![
                "model".to_string(),
                "context".to_string(),
                "style".to_string(),
            ],
            vec!["duration".to_string(), "tokens".to_string()],
        ],
    }
}

/// Get path to config file
/// Uses $XDG_CONFIG_HOME/claude/cc-statusline.json or ~/.config/claude/cc-statusline.json
fn get_config_path() -> PathBuf {
    let base = env::var("XDG_CONFIG_HOME").map_or_else(
        |_| {
            let home = get_home();
            if home.is_empty() {
                PathBuf::from(".config")
            } else {
                PathBuf::from(home).join(".config")
            }
        },
        PathBuf::from,
    );
    base.join("claude").join("cc-statusline.json")
}

/// Load configuration from file, returning default if missing or invalid
fn load_config() -> &'static Config {
    CONFIG.get_or_init(|| {
        let config_path = get_config_path();

        // If file doesn't exist, use defaults silently
        if !config_path.exists() {
            return default_config();
        }

        // Try to read and parse the file
        match fs::read_to_string(&config_path) {
            Ok(content) => match serde_json::from_str::<Config>(&content) {
                Ok(config) => {
                    // Validate config has at least one non-empty row
                    if config.rows.iter().any(|row| !row.is_empty()) {
                        config
                    } else {
                        eprintln!(
                            "cc-statusline: config at {} has no valid rows, using defaults",
                            config_path.display()
                        );
                        default_config()
                    }
                }
                Err(e) => {
                    eprintln!(
                        "cc-statusline: invalid config at {}: {e}",
                        config_path.display()
                    );
                    default_config()
                }
            },
            Err(e) => {
                eprintln!(
                    "cc-statusline: failed to read config at {}: {e}",
                    config_path.display()
                );
                default_config()
            }
        }
    })
}

/// Write default config to file (for --config-init)
/// Returns error if config file already exists (use --config-init --force to overwrite)
fn write_config_init(force: bool) -> io::Result<()> {
    let config_path = get_config_path();

    // Check if config already exists
    if config_path.exists() && !force {
        return Err(io::Error::other(format!(
            "config file already exists: {}\nUse --config-init --force to overwrite",
            config_path.display()
        )));
    }

    // Create parent directories if needed
    if let Some(parent) = config_path.parent() {
        fs::create_dir_all(parent)?;
    }

    // Write pretty-printed default config
    let config = default_config();
    let json = serde_json::to_string_pretty(&config)
        .map_err(|e| io::Error::other(format!("failed to serialize config: {e}")))?;

    fs::write(&config_path, json)?;
    println!("Created config file: {}", config_path.display());
    Ok(())
}

/// Get secure per-user cache directory
/// Uses $XDG_CACHE_HOME/cc-statusline or ~/.cache/cc-statusline
pub(crate) fn get_cache_dir() -> &'static PathBuf {
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
pub(crate) fn is_gh_available() -> bool {
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

/// Check if we're inside an SSH session by looking for SSH-related env vars
pub(crate) fn is_ssh_session() -> bool {
    env::var_os("SSH_CONNECTION").is_some() || env::var_os("SSH_CLIENT").is_some()
}

/// Get the system hostname via libc gethostname() (cached via OnceLock)
/// Strips the `.local` suffix (used by mDNS/Bonjour on Unix systems)
pub(crate) fn get_hostname() -> Option<&'static String> {
    HOSTNAME
        .get_or_init(|| {
            #[cfg(unix)]
            {
                let mut buf = [0u8; 256];
                let ret =
                    unsafe { libc::gethostname(buf.as_mut_ptr() as *mut libc::c_char, buf.len()) };
                if ret == 0 {
                    let len = buf.iter().position(|&b| b == 0).unwrap_or(buf.len());
                    std::str::from_utf8(&buf[..len]).ok().and_then(|name| {
                        let trimmed = name.strip_suffix(".local").unwrap_or(name);
                        if trimmed.is_empty() {
                            None
                        } else {
                            Some(trimmed.to_string())
                        }
                    })
                } else {
                    None
                }
            }
            #[cfg(not(unix))]
            {
                None
            }
        })
        .as_ref()
}

/// Get GitHub token for API authentication
/// Tries: 1) `GITHUB_TOKEN` env var, 2) `GH_TOKEN` env var, 3) git credential fill
pub(crate) fn get_github_token() -> Option<String> {
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

/// Best-effort cross-platform rename that overwrites the destination.
///
/// On Unix-like platforms this is typically atomic. On Windows, `fs::rename`
/// fails if the destination exists, so we remove the destination first and
/// then rename. This is *not* a truly atomic replacement on Windows, as
/// there is a brief window where the destination path does not exist.
pub(crate) fn atomic_rename(from: &Path, to: &Path) -> io::Result<()> {
    // On Windows, fs::rename fails if destination exists; remove it first.
    #[cfg(windows)]
    let _ = fs::remove_file(to);
    fs::rename(from, to)
}

/// Generate a unique hex string for temp file names
/// Uses timestamp + pid + atomic counter to avoid collisions within same process
pub(crate) fn unique_hex() -> String {
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

#[derive(Deserialize, Default)]
#[serde(default)]
pub(crate) struct ClaudeInput {
    pub(crate) cwd: Option<String>,
    pub(crate) model: Model,
    pub(crate) context_window: ContextWindow,
    pub(crate) cost: Cost,
    pub(crate) output_style: OutputStyle,
    pub(crate) workspace: Workspace,
    pub(crate) git: GitInput,
    pub(crate) pr: PrInput,
}

#[derive(Deserialize, Default)]
#[serde(default)]
pub(crate) struct Model {
    pub(crate) display_name: Option<String>,
}

#[derive(Deserialize, Default)]
#[serde(default)]
pub(crate) struct ContextWindow {
    pub(crate) remaining_percentage: Option<f64>,
    pub(crate) total_input_tokens: Option<u64>,
    pub(crate) total_output_tokens: Option<u64>,
}

#[derive(Deserialize, Default)]
#[serde(default)]
pub(crate) struct Cost {
    pub(crate) total_duration_ms: Option<u64>,
}

#[derive(Deserialize, Default)]
#[serde(default)]
pub(crate) struct OutputStyle {
    pub(crate) name: Option<String>,
}

#[derive(Deserialize, Default)]
#[serde(default)]
pub(crate) struct Workspace {
    pub(crate) project_dir: Option<String>,
    pub(crate) current_dir: Option<String>,
}

/// Git info from JSON input (for screenshots/testing)
#[derive(Deserialize, Default)]
#[serde(default)]
pub(crate) struct GitInput {
    pub(crate) branch: Option<String>,
    pub(crate) worktree: Option<String>,
    pub(crate) changed_files: Option<u32>,
    pub(crate) ahead: Option<u32>,
    pub(crate) behind: Option<u32>,
}

/// PR info from JSON input (for screenshots/testing)
#[derive(Deserialize, Default)]
#[serde(default)]
pub(crate) struct PrInput {
    pub(crate) number: Option<u32>,
    pub(crate) state: Option<String>,
    pub(crate) url: Option<String>,
    pub(crate) comments: Option<u32>,
    pub(crate) changed_files: Option<u32>,
    pub(crate) check_status: Option<String>,
}

fn main() {
    // Handle --version and --help before reading stdin
    let args: Vec<String> = env::args().collect();
    if args.len() > 1 {
        match args[1].as_str() {
            "--version" | "-V" => {
                println!("cc-statusline {}", env!("CARGO_PKG_VERSION"));
                return;
            }
            "--help" | "-h" => {
                println!("cc-statusline {}", env!("CARGO_PKG_VERSION"));
                println!();
                println!("A lightweight, fast status line for Claude Code CLI");
                println!();
                println!("USAGE:");
                println!("    cc-statusline [OPTIONS]");
                println!();
                println!("OPTIONS:");
                println!("    -h, --help              Print help information");
                println!("    -V, --version           Print version information");
                println!("    --config-init           Create default config file");
                println!("    --config-init --force   Overwrite existing config file");
                println!();
                println!("CONFIG:");
                println!("    {}", get_config_path().display());
                println!();
                println!("Reads JSON input from stdin for Claude Code integration.");
                return;
            }
            "--config-init" => {
                let force = args.get(2).is_some_and(|a| a == "--force");
                if let Err(e) = write_config_init(force) {
                    eprintln!("Error: {e}");
                    std::process::exit(1);
                }
                return;
            }
            _ => {}
        }
    }

    let mut input = String::with_capacity(4096);
    io::stdin().read_to_string(&mut input).unwrap_or_default();

    let data: ClaudeInput = serde_json::from_str(&input).unwrap_or_default();

    let current_dir: Cow<str> = match data.cwd.as_deref() {
        Some(dir) => Cow::Borrowed(dir),
        None => match data.workspace.current_dir.as_deref() {
            Some(dir) => Cow::Borrowed(dir),
            None => match data.workspace.project_dir.as_deref() {
                Some(dir) => Cow::Borrowed(dir),
                None => Cow::Owned(env::current_dir().unwrap().to_string_lossy().into_owned()),
            },
        },
    };

    // Skip filesystem detection if JSON provides git.branch
    let git_repo = if data.git.branch.is_some() {
        None
    } else {
        get_git_repo(&current_dir)
    };

    // Load config and render
    let config = load_config();
    let view = gather(&data, &current_dir, git_repo.as_ref());

    let stdout = io::stdout();
    let mut out = BufWriter::new(stdout.lock());
    write_rows(&mut out, config, &view);
    out.flush().unwrap_or_default();
}

#[cfg(test)]
mod tests {
    // These exercise the pure helpers re-exported from the library crate.
    use cc_statusline::{
        abbreviate_path, hash_path, parse_github_url, percent_encode, shell_escape,
    };

    #[test]
    fn hash_path_deterministic() {
        let path = "/home/user/project";
        assert_eq!(hash_path(path), hash_path(path));
    }

    #[test]
    fn hash_path_different_inputs() {
        assert_ne!(
            hash_path("/home/user/project1"),
            hash_path("/home/user/project2")
        );
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

    #[test]
    fn parse_ssh_url() {
        let result = parse_github_url("git@github.com:owner/repo.git");
        assert_eq!(result, Some(("owner".to_string(), "repo".to_string())));
    }

    #[test]
    fn parse_ssh_url_without_git_suffix() {
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

    #[test]
    fn path_within_width_unchanged() {
        let path = "~/short";
        assert_eq!(abbreviate_path(path, 50).as_ref(), path);
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
        // Single segment can't be abbreviated further
        assert_eq!(abbreviate_path(path, 5).as_ref(), path);
    }

    #[test]
    fn root_path() {
        let path = "/";
        assert_eq!(abbreviate_path(path, 50).as_ref(), path);
    }

    #[test]
    fn two_segment_path() {
        let path = "~/project";
        // Should keep both segments as much as possible
        assert!(abbreviate_path(path, 5).contains("project"));
    }

    #[test]
    fn tilde_home_preserved() {
        let path = "~/a/b/c/d/project";
        // Tilde should be preserved as first char abbreviation
        assert!(abbreviate_path(path, 20).starts_with('~'));
    }

    #[test]
    fn shell_escape_single_quotes() {
        assert_eq!(shell_escape("it's a test"), "'it'\\''s a test'");
    }

    #[test]
    fn shell_escape_empty_string() {
        assert_eq!(shell_escape(""), "''");
    }

    #[test]
    fn shell_escape_no_escape_needed() {
        assert_eq!(shell_escape("simple"), "'simple'");
    }

    #[test]
    fn shell_escape_special_chars() {
        // Special shell characters should be safely escaped inside single quotes
        assert_eq!(shell_escape("$HOME && rm -rf /"), "'$HOME && rm -rf /'");
    }

    #[test]
    fn shell_escape_multiple_quotes() {
        assert_eq!(shell_escape("it's Bob's"), "'it'\\''s Bob'\\''s'");
    }

    #[test]
    fn percent_encode_spaces() {
        assert_eq!(percent_encode("hello world"), "hello%20world");
    }

    #[test]
    fn percent_encode_special_chars() {
        assert_eq!(percent_encode("test#branch"), "test%23branch");
    }

    #[test]
    fn percent_encode_unreserved_chars_unchanged() {
        // RFC 3986 unreserved: ALPHA / DIGIT / "-" / "." / "_" / "~"
        assert_eq!(percent_encode("azAZ09-._~"), "azAZ09-._~");
    }

    #[test]
    fn percent_encode_slash() {
        assert_eq!(percent_encode("path/to/file"), "path%2Fto%2Ffile");
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
        assert_eq!(percent_encode(""), "");
    }
}
