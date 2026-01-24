.PHONY: build release screenshots clean

# Screenshot settings
FREEZE_WIDTH := 60
FREEZE_FLAGS := -W $(FREEZE_WIDTH) --window=false --padding 10,20
DOCS_DIR := docs/screenshots

build:
	cargo build

release:
	cargo build --release

clean:
	cargo clean

# Generate all screenshot permutations as PNGs
screenshots: release
	@mkdir -p $(DOCS_DIR)
	@echo "Generating screenshots..."
	@echo '{"workspace":{"project_dir":"/app","current_dir":"/tmp"}}' | ./target/release/cc-statusline | freeze $(FREEZE_FLAGS) -o $(DOCS_DIR)/01-no-git.png
	@echo '{"workspace":{"project_dir":"/app"},"git":{"branch":"main"}}' | ./target/release/cc-statusline | freeze $(FREEZE_FLAGS) -o $(DOCS_DIR)/02-git-branch.png
	@echo '{"workspace":{"project_dir":"/app"},"git":{"branch":"feat/login","changed_files":3}}' | ./target/release/cc-statusline | freeze $(FREEZE_FLAGS) -o $(DOCS_DIR)/03-git-changed-files.png
	@echo '{"workspace":{"project_dir":"/app"},"git":{"branch":"local-experiment","changed_files":1}}' | ./target/release/cc-statusline | freeze $(FREEZE_FLAGS) -o $(DOCS_DIR)/04-git-local-only.png
	@echo '{"workspace":{"project_dir":"/app"},"git":{"branch":"main","changed_files":2,"ahead":3,"behind":1}}' | ./target/release/cc-statusline | freeze $(FREEZE_FLAGS) -o $(DOCS_DIR)/05-git-ahead-behind.png
	@echo '{"workspace":{"project_dir":"/app"},"git":{"branch":"feat/push-me","ahead":5}}' | ./target/release/cc-statusline | freeze $(FREEZE_FLAGS) -o $(DOCS_DIR)/06-git-ahead-only.png
	@echo '{"workspace":{"project_dir":"/app"},"git":{"branch":"main","behind":12}}' | ./target/release/cc-statusline | freeze $(FREEZE_FLAGS) -o $(DOCS_DIR)/07-git-behind-only.png
	@echo '{"workspace":{"project_dir":"/app"},"git":{"branch":"feat/login","changed_files":2},"pr":{"number":42,"state":"open","changed_files":5,"check_status":"passed"}}' | ./target/release/cc-statusline | freeze $(FREEZE_FLAGS) -o $(DOCS_DIR)/08-pr-open-passed.png
	@echo '{"workspace":{"project_dir":"/app"},"git":{"branch":"feat/login","changed_files":2},"pr":{"number":42,"state":"open","check_status":"pending"}}' | ./target/release/cc-statusline | freeze $(FREEZE_FLAGS) -o $(DOCS_DIR)/09-pr-open-pending.png
	@echo '{"workspace":{"project_dir":"/app"},"git":{"branch":"feat/login","changed_files":2},"pr":{"number":42,"state":"open","check_status":"failed"}}' | ./target/release/cc-statusline | freeze $(FREEZE_FLAGS) -o $(DOCS_DIR)/10-pr-open-failed.png
	@echo '{"workspace":{"project_dir":"/app"},"git":{"branch":"main"},"pr":{"number":41,"state":"merged","check_status":"passed"}}' | ./target/release/cc-statusline | freeze $(FREEZE_FLAGS) -o $(DOCS_DIR)/11-pr-merged.png
	@echo '{"workspace":{"project_dir":"/app"},"git":{"branch":"main"},"pr":{"number":40,"state":"closed"}}' | ./target/release/cc-statusline | freeze $(FREEZE_FLAGS) -o $(DOCS_DIR)/12-pr-closed.png
	@echo '{"workspace":{"project_dir":"/app"},"git":{"branch":"main"},"model":{"display_name":"Opus 4.5"},"context_window":{"remaining_percentage":47}}' | ./target/release/cc-statusline | freeze $(FREEZE_FLAGS) -o $(DOCS_DIR)/13-model-context.png
	@echo '{"workspace":{"project_dir":"/app"},"git":{"branch":"main"},"model":{"display_name":"Sonnet 4"},"context_window":{"remaining_percentage":82},"output_style":{"name":"verbose"}}' | ./target/release/cc-statusline | freeze $(FREEZE_FLAGS) -o $(DOCS_DIR)/14-model-context-style.png
	@echo '{"workspace":{"project_dir":"/app"},"git":{"branch":"feat/hourly-forecast","changed_files":3},"pr":{"number":42,"state":"open","changed_files":5,"check_status":"passed"},"model":{"display_name":"Opus 4.5"},"context_window":{"remaining_percentage":47}}' | ./target/release/cc-statusline | freeze $(FREEZE_FLAGS) -o $(DOCS_DIR)/15-full.png
	@echo '{"workspace":{"project_dir":"/app"},"git":{"branch":"feat/api","changed_files":7},"pr":{"number":99,"state":"open","changed_files":12,"check_status":"pending"},"model":{"display_name":"Opus 4.5"},"context_window":{"remaining_percentage":23,"total_input_tokens":150000,"total_output_tokens":45000},"cost":{"total_duration_ms":1860000}}' | ./target/release/cc-statusline | freeze $(FREEZE_FLAGS) -o $(DOCS_DIR)/16-full-duration-tokens.png
	@echo "Screenshots saved to $(DOCS_DIR)/"
	@ls -la $(DOCS_DIR)/*.png
