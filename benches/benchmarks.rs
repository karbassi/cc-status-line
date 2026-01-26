//! Benchmarks for cc-statusline
//!
//! Run with: cargo bench

use criterion::{Criterion, Throughput, black_box, criterion_group, criterion_main};
use std::io::Write;
use std::process::{Command, Stdio};

/// Benchmark the full binary startup with minimal JSON input
fn bench_startup_minimal(c: &mut Criterion) {
    let binary = env!("CARGO_BIN_EXE_cc-statusline");

    c.bench_function("startup_minimal", |b| {
        b.iter(|| {
            let mut child = Command::new(binary)
                .stdin(Stdio::piped())
                .stdout(Stdio::piped())
                .stderr(Stdio::null())
                .spawn()
                .expect("failed to spawn");

            child
                .stdin
                .take()
                .unwrap()
                .write_all(b"{}")
                .expect("failed to write");

            let output = child.wait_with_output().expect("failed to wait");
            black_box(output.stdout)
        })
    });
}

/// Benchmark with full JSON input (simulates real Claude Code usage)
fn bench_startup_full_json(c: &mut Criterion) {
    let binary = env!("CARGO_BIN_EXE_cc-statusline");

    let json_input = r#"{
        "model": {"display_name": "Claude Opus 4.5"},
        "context_window": {
            "remaining_percentage": 75.5,
            "total_input_tokens": 50000,
            "total_output_tokens": 25000
        },
        "cost": {"total_duration_ms": 125000},
        "output_style": {"name": "verbose"},
        "workspace": {
            "project_dir": "/Users/test/project",
            "current_dir": "/Users/test/project/src/components"
        },
        "git": {
            "branch": "feature-branch",
            "worktree": null,
            "changed_files": 5,
            "ahead": 2,
            "behind": 1
        },
        "pr": {
            "number": 42,
            "state": "open",
            "url": "https://github.com/owner/repo/pull/42",
            "comments": 3,
            "changed_files": 10,
            "check_status": "passed"
        }
    }"#;

    c.bench_function("startup_full_json", |b| {
        b.iter(|| {
            let mut child = Command::new(binary)
                .stdin(Stdio::piped())
                .stdout(Stdio::piped())
                .stderr(Stdio::null())
                .spawn()
                .expect("failed to spawn");

            child
                .stdin
                .take()
                .unwrap()
                .write_all(json_input.as_bytes())
                .expect("failed to write");

            let output = child.wait_with_output().expect("failed to wait");
            black_box(output.stdout)
        })
    });
}

// =============================================================================
// Pure function benchmarks (duplicated from main.rs for benchmarking)
// =============================================================================

fn hash_path(path: &str) -> u64 {
    path.bytes().fold(0u64, |acc, b| {
        acc.wrapping_mul(31).wrapping_add(u64::from(b))
    })
}

fn bench_hash_path(c: &mut Criterion) {
    let short_path = "/home/user/project";
    let long_path = "/home/user/very/deeply/nested/directory/structure/with/many/segments/project";

    let mut group = c.benchmark_group("hash_path");
    group.throughput(Throughput::Elements(1));

    group.bench_function("short_path", |b| {
        b.iter(|| hash_path(black_box(short_path)))
    });

    group.bench_function("long_path", |b| b.iter(|| hash_path(black_box(long_path))));

    group.finish();
}

fn shell_escape(s: &str) -> String {
    format!("'{}'", s.replace('\'', "'\\''"))
}

fn bench_shell_escape(c: &mut Criterion) {
    let simple = "simple-string";
    let with_quotes = "it's Bob's file";
    let complex = "path with 'quotes' and $variables && commands";

    let mut group = c.benchmark_group("shell_escape");

    group.bench_function("simple", |b| b.iter(|| shell_escape(black_box(simple))));

    group.bench_function("with_quotes", |b| {
        b.iter(|| shell_escape(black_box(with_quotes)))
    });

    group.bench_function("complex", |b| b.iter(|| shell_escape(black_box(complex))));

    group.finish();
}

fn percent_encode(s: &str) -> String {
    use std::fmt::Write;
    let mut result = String::with_capacity(s.len() * 3);
    for byte in s.bytes() {
        match byte {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'.' | b'_' | b'~' => {
                result.push(byte as char);
            }
            _ => {
                result.push('%');
                let _ = write!(result, "{byte:02X}");
            }
        }
    }
    result
}

fn bench_percent_encode(c: &mut Criterion) {
    let simple = "simple-string";
    let with_spaces = "hello world test";
    let unicode = "日本語テスト";
    let branch = "feature/test#123";

    let mut group = c.benchmark_group("percent_encode");

    group.bench_function("simple", |b| b.iter(|| percent_encode(black_box(simple))));

    group.bench_function("with_spaces", |b| {
        b.iter(|| percent_encode(black_box(with_spaces)))
    });

    group.bench_function("unicode", |b| b.iter(|| percent_encode(black_box(unicode))));

    group.bench_function("branch_name", |b| {
        b.iter(|| percent_encode(black_box(branch)))
    });

    group.finish();
}

fn parse_github_url(url: &str) -> Option<(String, String)> {
    if let Some(rest) = url.strip_prefix("git@github.com:") {
        let path = rest.trim_end_matches(".git");
        let mut parts = path.splitn(2, '/');
        let owner = parts.next()?.to_string();
        let repo = parts.next()?.to_string();
        if !owner.is_empty() && !repo.is_empty() {
            return Some((owner, repo));
        }
    }

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

fn bench_parse_github_url(c: &mut Criterion) {
    let ssh_url = "git@github.com:owner/repo.git";
    let https_url = "https://github.com/owner/repo.git";
    let invalid_url = "https://gitlab.com/owner/repo.git";

    let mut group = c.benchmark_group("parse_github_url");

    group.bench_function("ssh", |b| b.iter(|| parse_github_url(black_box(ssh_url))));

    group.bench_function("https", |b| {
        b.iter(|| parse_github_url(black_box(https_url)))
    });

    group.bench_function("invalid", |b| {
        b.iter(|| parse_github_url(black_box(invalid_url)))
    });

    group.finish();
}

use std::borrow::Cow;

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

fn bench_abbreviate_path(c: &mut Criterion) {
    let short_path = "~/project";
    let medium_path = "~/code/rust/cc-status-line";
    let long_path = "~/very/deeply/nested/directory/structure/project";

    let mut group = c.benchmark_group("abbreviate_path");

    group.bench_function("short_no_abbrev", |b| {
        b.iter(|| abbreviate_path(black_box(short_path), 50))
    });

    group.bench_function("medium_no_abbrev", |b| {
        b.iter(|| abbreviate_path(black_box(medium_path), 50))
    });

    group.bench_function("long_needs_abbrev", |b| {
        b.iter(|| abbreviate_path(black_box(long_path), 30))
    });

    group.finish();
}

criterion_group!(
    benches,
    bench_startup_minimal,
    bench_startup_full_json,
    bench_hash_path,
    bench_shell_escape,
    bench_percent_encode,
    bench_parse_github_url,
    bench_abbreviate_path,
);

criterion_main!(benches);
