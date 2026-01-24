.PHONY: build release screenshots clean

# Screenshot settings
FREEZE_FLAGS := --window --font.family "Menlo" --font.size 14 --shadow.blur 20 --shadow.y 10 --border.radius 10 -m 20
DOCS_DIR := docs/screenshots

# Consistent data for screenshots
PROJECT := ~/projects/weather-app
BASE := "model":{"display_name":"Opus 4.5"},"context_window":{"remaining_percentage":47,"total_input_tokens":125000,"total_output_tokens":42000},"cost":{"total_duration_ms":1860000}

build:
	cargo build

release:
	cargo build --release

clean:
	cargo clean

# Generate all screenshot permutations as PNGs
screenshots: release
	@rm -rf $(DOCS_DIR)
	@mkdir -p $(DOCS_DIR)
	@echo "Generating screenshots..."
	@echo '{"workspace":{"project_dir":"$(PROJECT)","current_dir":"$(PROJECT)"},$(BASE)}' | ./target/release/cc-statusline | freeze $(FREEZE_FLAGS) -o $(DOCS_DIR)/01-no-git.png
	@echo '{"workspace":{"project_dir":"$(PROJECT)","current_dir":"$(PROJECT)"},"git":{"branch":"main"},$(BASE)}' | ./target/release/cc-statusline | freeze $(FREEZE_FLAGS) -o $(DOCS_DIR)/02-git-branch.png
	@echo '{"workspace":{"project_dir":"$(PROJECT)","current_dir":"$(PROJECT)"},"git":{"branch":"feat/login","changed_files":3},$(BASE)}' | ./target/release/cc-statusline | freeze $(FREEZE_FLAGS) -o $(DOCS_DIR)/03-git-changed-files.png
	@echo '{"workspace":{"project_dir":"$(PROJECT)","current_dir":"$(PROJECT)"},"git":{"branch":"local-experiment","changed_files":1},$(BASE)}' | ./target/release/cc-statusline | freeze $(FREEZE_FLAGS) -o $(DOCS_DIR)/04-git-local-only.png
	@echo '{"workspace":{"project_dir":"$(PROJECT)","current_dir":"$(PROJECT)"},"git":{"branch":"main","changed_files":2,"ahead":3,"behind":1},$(BASE)}' | ./target/release/cc-statusline | freeze $(FREEZE_FLAGS) -o $(DOCS_DIR)/05-git-ahead-behind.png
	@echo '{"workspace":{"project_dir":"$(PROJECT)","current_dir":"$(PROJECT)"},"git":{"branch":"feat/push-me","ahead":5},$(BASE)}' | ./target/release/cc-statusline | freeze $(FREEZE_FLAGS) -o $(DOCS_DIR)/06-git-ahead-only.png
	@echo '{"workspace":{"project_dir":"$(PROJECT)","current_dir":"$(PROJECT)"},"git":{"branch":"main","behind":12},$(BASE)}' | ./target/release/cc-statusline | freeze $(FREEZE_FLAGS) -o $(DOCS_DIR)/07-git-behind-only.png
	@echo '{"workspace":{"project_dir":"$(PROJECT)","current_dir":"$(PROJECT)"},"git":{"branch":"main","worktree":"hotfix-v2"},$(BASE)}' | ./target/release/cc-statusline | freeze $(FREEZE_FLAGS) -o $(DOCS_DIR)/08-git-worktree.png
	@echo '{"workspace":{"project_dir":"$(PROJECT)","current_dir":"$(PROJECT)/src/components/dashboard/widgets"},"git":{"branch":"main"},$(BASE)}' | ./target/release/cc-statusline | freeze $(FREEZE_FLAGS) -o $(DOCS_DIR)/09-fish-path.png
	@echo '{"workspace":{"project_dir":"$(PROJECT)","current_dir":"$(PROJECT)"},"git":{"branch":"feat/login","changed_files":2},"pr":{"number":42,"state":"open","changed_files":5,"check_status":"passed"},$(BASE)}' | ./target/release/cc-statusline | freeze $(FREEZE_FLAGS) -o $(DOCS_DIR)/10-pr-open-passed.png
	@echo '{"workspace":{"project_dir":"$(PROJECT)","current_dir":"$(PROJECT)"},"git":{"branch":"feat/login","changed_files":2},"pr":{"number":42,"state":"open","check_status":"pending"},$(BASE)}' | ./target/release/cc-statusline | freeze $(FREEZE_FLAGS) -o $(DOCS_DIR)/11-pr-open-pending.png
	@echo '{"workspace":{"project_dir":"$(PROJECT)","current_dir":"$(PROJECT)"},"git":{"branch":"feat/login","changed_files":2},"pr":{"number":42,"state":"open","check_status":"failed"},$(BASE)}' | ./target/release/cc-statusline | freeze $(FREEZE_FLAGS) -o $(DOCS_DIR)/12-pr-open-failed.png
	@echo '{"workspace":{"project_dir":"$(PROJECT)","current_dir":"$(PROJECT)"},"git":{"branch":"main"},"pr":{"number":41,"state":"merged","check_status":"passed"},$(BASE)}' | ./target/release/cc-statusline | freeze $(FREEZE_FLAGS) -o $(DOCS_DIR)/13-pr-merged.png
	@echo '{"workspace":{"project_dir":"$(PROJECT)","current_dir":"$(PROJECT)"},"git":{"branch":"main"},"pr":{"number":40,"state":"closed"},$(BASE)}' | ./target/release/cc-statusline | freeze $(FREEZE_FLAGS) -o $(DOCS_DIR)/14-pr-closed.png
	@echo '{"workspace":{"project_dir":"$(PROJECT)","current_dir":"$(PROJECT)"},"git":{"branch":"main"},"model":{"display_name":"Sonnet 4"},"context_window":{"remaining_percentage":82,"total_input_tokens":125000,"total_output_tokens":42000},"output_style":{"name":"verbose"},"cost":{"total_duration_ms":1860000}}' | ./target/release/cc-statusline | freeze $(FREEZE_FLAGS) -o $(DOCS_DIR)/15-output-style.png
	@echo '{"workspace":{"project_dir":"$(PROJECT)","current_dir":"$(PROJECT)"},"git":{"branch":"feat/hourly-forecast","changed_files":3},"pr":{"number":42,"state":"open","changed_files":5,"check_status":"passed"},$(BASE)}' | ./target/release/cc-statusline | freeze $(FREEZE_FLAGS) -o $(DOCS_DIR)/16-full.png
	@echo "Screenshots saved to $(DOCS_DIR)/"
	@ls -la $(DOCS_DIR)/*.png
