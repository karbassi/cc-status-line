//! cc-statusline library
//!
//! This module exposes pure functions for use in benchmarks and tests.
//! The main binary logic remains in main.rs.

use std::borrow::Cow;
use std::fmt::Write;

/// Hash a path string to a u64 using a simple polynomial hash.
/// Used for generating unique cache file names.
pub fn hash_path(path: &str) -> u64 {
    path.bytes().fold(0u64, |acc, b| {
        acc.wrapping_mul(31).wrapping_add(u64::from(b))
    })
}

/// Shell-escape a string by wrapping in single quotes and escaping embedded single quotes.
/// This is the POSIX-safe way to escape arbitrary strings for shell arguments.
pub fn shell_escape(s: &str) -> String {
    format!("'{}'", s.replace('\'', "'\\''"))
}

/// Percent-encode a string for use in URLs.
/// Encodes characters that are not unreserved per RFC 3986.
pub fn percent_encode(s: &str) -> String {
    let mut result = String::with_capacity(s.len() * 3);
    for byte in s.bytes() {
        match byte {
            // Unreserved characters (RFC 3986): ALPHA / DIGIT / "-" / "." / "_" / "~"
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'.' | b'_' | b'~' => {
                result.push(byte as char);
            }
            // Everything else gets percent-encoded
            _ => {
                result.push('%');
                let _ = write!(result, "{byte:02X}");
            }
        }
    }
    result
}

/// Parse owner/repo from a GitHub URL.
/// Validates the host is exactly `github.com` to avoid false positives.
///
/// Handles:
/// - SSH format: `git@github.com:owner/repo.git`
/// - HTTPS format: `https://github.com/owner/repo.git`
pub fn parse_github_url(url: &str) -> Option<(String, String)> {
    // SSH format: git@github.com:owner/repo.git (exact prefix match)
    if let Some(rest) = url.strip_prefix("git@github.com:") {
        let path = rest.trim_end_matches(".git");
        let mut parts = path.splitn(2, '/');
        let owner = parts.next()?.to_string();
        let repo = parts.next()?.to_string();
        if !owner.is_empty() && !repo.is_empty() {
            return Some((owner, repo));
        }
    }

    // HTTPS format: https://github.com/owner/repo.git
    // Validate host is exactly github.com (not notgithub.com, etc.)
    let url_lower = url.to_lowercase();
    if url_lower.starts_with("https://github.com/") || url_lower.starts_with("http://github.com/") {
        let proto_end = url.find("://")? + 3;
        let path_start = proto_end + "github.com/".len();
        if url.len() > path_start {
            let path = url[path_start..].trim_end_matches(".git");
            let mut parts = path.splitn(2, '/');
            let owner = parts.next()?.to_string();
            let repo = parts.next()?.to_string();
            if !owner.is_empty() && !repo.is_empty() {
                return Some((owner, repo));
            }
        }
    }

    None
}

/// Abbreviate a filesystem path to fit within a given width.
///
/// Strategy:
/// - If path fits, return as-is
/// - Otherwise, abbreviate parent directories to first character
/// - Always preserve the last two segments (parent/leaf) if possible
pub fn abbreviate_path(path: &str, max_width: usize) -> Cow<'_, str> {
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
        for &start in seg_starts.iter().take(seg_count.saturating_sub(2)) {
            if start < bytes.len() && bytes[start] != b'/' {
                result.push(bytes[start] as char);
                result.push('/');
            }
        }
        result.push_str(parent_seg);
        result.push('/');
        result.push_str(last_seg);
    } else {
        for &start in seg_starts.iter().take(seg_count - 1) {
            if start < bytes.len() && bytes[start] != b'/' {
                result.push(bytes[start] as char);
                result.push('/');
            }
        }
        result.push_str(last_seg);
    }

    Cow::Owned(result)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_hash_path_deterministic() {
        let path = "/home/user/project";
        assert_eq!(hash_path(path), hash_path(path));
    }

    #[test]
    fn test_shell_escape_quotes() {
        let result = shell_escape("it's a test");
        assert_eq!(result, "'it'\\''s a test'");
    }

    #[test]
    fn test_percent_encode_special() {
        let result = percent_encode("hello world");
        assert_eq!(result, "hello%20world");
    }

    #[test]
    fn test_parse_github_ssh() {
        let result = parse_github_url("git@github.com:owner/repo.git");
        assert_eq!(result, Some(("owner".to_string(), "repo".to_string())));
    }

    #[test]
    fn test_abbreviate_short_path() {
        let path = "~/short";
        let result = abbreviate_path(path, 50);
        assert_eq!(result.as_ref(), path);
    }
}
