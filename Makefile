.PHONY: build release screenshots clean

build:
	cargo build

release:
	cargo build --release

clean:
	cargo clean

# Screenshot settings
DOCS := docs/screenshots
FREEZE := freeze --window --font.family "Menlo" --font.size 14 --shadow.blur 20 --shadow.y 10 --border.radius 10 -m 20
RUN := ./target/release/cc-statusline | $(FREEZE) -o $(DOCS)

# Common JSON fragments
P := ~/projects/weather-app
W := "workspace":{"project_dir":"$(P)","current_dir":"$(P)"}
M := "model":{"display_name":"Opus 4.5"},"context_window":{"remaining_percentage":47,"total_input_tokens":125000,"total_output_tokens":42000},"cost":{"total_duration_ms":1860000}

screenshots: release
	@rm -rf $(DOCS) && mkdir -p $(DOCS)
	@echo "Generating screenshots..."
	@echo '{$(W),$(M)}' | $(RUN)/01-no-git.png
	@echo '{$(W),"git":{"branch":"main"},$(M)}' | $(RUN)/02-git-branch.png
	@echo '{$(W),"git":{"branch":"feat/login","changed_files":3},$(M)}' | $(RUN)/03-git-changed-files.png
	@echo '{$(W),"git":{"branch":"local-experiment","changed_files":1},$(M)}' | $(RUN)/04-git-local-only.png
	@echo '{$(W),"git":{"branch":"main","changed_files":2,"ahead":3,"behind":1},$(M)}' | $(RUN)/05-git-ahead-behind.png
	@echo '{$(W),"git":{"branch":"feat/push-me","ahead":5},$(M)}' | $(RUN)/06-git-ahead-only.png
	@echo '{$(W),"git":{"branch":"main","behind":12},$(M)}' | $(RUN)/07-git-behind-only.png
	@echo '{$(W),"git":{"branch":"main","worktree":"hotfix-v2"},$(M)}' | $(RUN)/08-git-worktree.png
	@echo '{"workspace":{"project_dir":"$(P)","current_dir":"$(P)/src/components/dashboard/widgets"},"git":{"branch":"main"},$(M)}' | $(RUN)/09-fish-path.png
	@echo '{$(W),"git":{"branch":"feat/login","changed_files":2},"pr":{"number":42,"state":"open","changed_files":5,"check_status":"passed"},$(M)}' | $(RUN)/10-pr-open-passed.png
	@echo '{$(W),"git":{"branch":"feat/login","changed_files":2},"pr":{"number":42,"state":"open","check_status":"pending"},$(M)}' | $(RUN)/11-pr-open-pending.png
	@echo '{$(W),"git":{"branch":"feat/login","changed_files":2},"pr":{"number":42,"state":"open","check_status":"failed"},$(M)}' | $(RUN)/12-pr-open-failed.png
	@echo '{$(W),"git":{"branch":"main"},"pr":{"number":41,"state":"merged","check_status":"passed"},$(M)}' | $(RUN)/13-pr-merged.png
	@echo '{$(W),"git":{"branch":"main"},"pr":{"number":40,"state":"closed"},$(M)}' | $(RUN)/14-pr-closed.png
	@echo '{$(W),"git":{"branch":"main"},"model":{"display_name":"Sonnet 4"},"context_window":{"remaining_percentage":82,"total_input_tokens":125000,"total_output_tokens":42000},"output_style":{"name":"verbose"},"cost":{"total_duration_ms":1860000}}' | $(RUN)/15-output-style.png
	@echo '{$(W),"git":{"branch":"feat/hourly-forecast","changed_files":3},"pr":{"number":42,"state":"open","changed_files":5,"check_status":"passed"},$(M)}' | $(RUN)/16-full.png
	@echo "Done. $(DOCS)/"
