.PHONY: build release screenshots clean

build:
	cargo build

release:
	cargo build --release

clean:
	cargo clean

# Generate all screenshot permutations - deterministic output without filesystem detection
screenshots: release
	@echo "=== No git ==="
	@echo '{"workspace":{"project_dir":"/app","current_dir":"/tmp"}}' | ./target/release/cc-statusline
	@echo
	@echo "=== Git: branch only ==="
	@echo '{"workspace":{"project_dir":"/app"},"git":{"branch":"main"}}' | ./target/release/cc-statusline
	@echo
	@echo "=== Git: branch + changed files ==="
	@echo '{"workspace":{"project_dir":"/app"},"git":{"branch":"feat/login","changed_files":3}}' | ./target/release/cc-statusline
	@echo
	@echo "=== Git: local only (no remote) ==="
	@echo '{"workspace":{"project_dir":"/app"},"git":{"branch":"local-experiment","changed_files":1}}' | ./target/release/cc-statusline
	@echo
	@echo "=== Git: with remote (ahead/behind) ==="
	@echo '{"workspace":{"project_dir":"/app"},"git":{"branch":"main","changed_files":2,"ahead":3,"behind":1}}' | ./target/release/cc-statusline
	@echo
	@echo "=== Git: ahead only ==="
	@echo '{"workspace":{"project_dir":"/app"},"git":{"branch":"feat/push-me","ahead":5}}' | ./target/release/cc-statusline
	@echo
	@echo "=== Git: behind only ==="
	@echo '{"workspace":{"project_dir":"/app"},"git":{"branch":"main","behind":12}}' | ./target/release/cc-statusline
	@echo
	@echo "=== PR: open + checks passed ==="
	@echo '{"workspace":{"project_dir":"/app"},"git":{"branch":"feat/login","changed_files":2},"pr":{"number":42,"state":"open","changed_files":5,"check_status":"passed"}}' | ./target/release/cc-statusline
	@echo
	@echo "=== PR: open + checks pending ==="
	@echo '{"workspace":{"project_dir":"/app"},"git":{"branch":"feat/login","changed_files":2},"pr":{"number":42,"state":"open","check_status":"pending"}}' | ./target/release/cc-statusline
	@echo
	@echo "=== PR: open + checks failed ==="
	@echo '{"workspace":{"project_dir":"/app"},"git":{"branch":"feat/login","changed_files":2},"pr":{"number":42,"state":"open","check_status":"failed"}}' | ./target/release/cc-statusline
	@echo
	@echo "=== PR: merged ==="
	@echo '{"workspace":{"project_dir":"/app"},"git":{"branch":"main"},"pr":{"number":41,"state":"merged","check_status":"passed"}}' | ./target/release/cc-statusline
	@echo
	@echo "=== PR: closed ==="
	@echo '{"workspace":{"project_dir":"/app"},"git":{"branch":"main"},"pr":{"number":40,"state":"closed"}}' | ./target/release/cc-statusline
	@echo
	@echo "=== Model + context ==="
	@echo '{"workspace":{"project_dir":"/app"},"git":{"branch":"main"},"model":{"display_name":"Opus 4.5"},"context_window":{"remaining_percentage":47}}' | ./target/release/cc-statusline
	@echo
	@echo "=== Model + context + output style ==="
	@echo '{"workspace":{"project_dir":"/app"},"git":{"branch":"main"},"model":{"display_name":"Sonnet 4"},"context_window":{"remaining_percentage":82},"output_style":{"name":"verbose"}}' | ./target/release/cc-statusline
	@echo
	@echo "=== Full: all fields ==="
	@echo '{"workspace":{"project_dir":"/app"},"git":{"branch":"feat/hourly-forecast","changed_files":3},"pr":{"number":42,"state":"open","changed_files":5,"check_status":"passed"},"model":{"display_name":"Opus 4.5"},"context_window":{"remaining_percentage":47}}' | ./target/release/cc-statusline
	@echo
	@echo "=== Full: with duration + tokens ==="
	@echo '{"workspace":{"project_dir":"/app"},"git":{"branch":"feat/api","changed_files":7},"pr":{"number":99,"state":"open","changed_files":12,"check_status":"pending"},"model":{"display_name":"Opus 4.5"},"context_window":{"remaining_percentage":23,"total_input_tokens":150000,"total_output_tokens":45000},"cost":{"total_duration_ms":1860000}}' | ./target/release/cc-statusline
