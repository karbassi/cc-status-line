.PHONY: build release screenshot screenshots clean

build:
	cargo build

release:
	cargo build --release

screenshot: release
	termshot -f screenshot.png -- bash -c 'echo "{\"workspace\":{\"project_dir\":\"/Users/ali.karbassi/Projects/personal/cc-status-line\",\"current_dir\":\"/Users/ali.karbassi/Projects/personal/cc-status-line\"},\"model\":{\"display_name\":\"Opus 4.5\"},\"context_window\":{\"remaining_percentage\":47,\"total_input_tokens\":92000,\"total_output_tokens\":64000},\"cost\":{\"total_duration_ms\":5640000}}" | ./target/release/cc-statusline'

screenshots: release
	./scripts/screenshots.sh

clean:
	cargo clean
	rm -f screenshot.png
	rm -rf docs/screenshots
