#!/bin/bash
set -e

mkdir -p docs/screenshots

BIN="./target/release/cc-statusline"
REPO_DIR="/Users/ali.karbassi/Projects/personal/cc-status-line"

# Ensure PR cache is populated first
echo "Populating PR cache..."
cd "$REPO_DIR"
echo '{}' | $BIN > /dev/null
sleep 3

# 1. Minimal - just project and path (no git)
echo "Generating 01-minimal.png..."
termshot -f docs/screenshots/01-minimal.png -- bash -c "
  cd /tmp
  echo '{\"workspace\":{\"project_dir\":\"/tmp/my-app\",\"current_dir\":\"/tmp/my-app\"}}' | $REPO_DIR/$BIN
"

# 2. With git branch only (main branch, no PR)
echo "Generating 02-branch.png..."
termshot -f docs/screenshots/02-branch.png -- bash -c "
  cd $REPO_DIR
  git checkout main 2>/dev/null
  rm -f /tmp/cc-pr-*.cache /tmp/cc-gitpath-*.cache 2>/dev/null
  echo '{\"workspace\":{\"project_dir\":\"$REPO_DIR\",\"current_dir\":\"$REPO_DIR\"}}' | $BIN
  git checkout feat/pr-status-line 2>/dev/null
"

# 3. With PR info
echo "Generating 03-pr.png..."
# Repopulate cache after branch switch
echo '{}' | $BIN > /dev/null
sleep 3
termshot -f docs/screenshots/03-pr.png -- bash -c "
  cd $REPO_DIR
  echo '{\"workspace\":{\"project_dir\":\"$REPO_DIR\",\"current_dir\":\"$REPO_DIR\"}}' | $BIN
"

# 4. With model info only (no git)
echo "Generating 04-model.png..."
termshot -f docs/screenshots/04-model.png -- bash -c "
  cd /tmp
  echo '{\"workspace\":{\"project_dir\":\"/tmp/my-app\",\"current_dir\":\"/tmp/my-app\"},\"model\":{\"display_name\":\"Opus 4.5\"},\"context_window\":{\"remaining_percentage\":72}}' | $REPO_DIR/$BIN
"

# 5. Full status line
echo "Generating 05-full.png..."
termshot -f docs/screenshots/05-full.png -- bash -c "
  cd $REPO_DIR
  echo '{\"workspace\":{\"project_dir\":\"$REPO_DIR\",\"current_dir\":\"$REPO_DIR\"},\"model\":{\"display_name\":\"Opus 4.5\"},\"context_window\":{\"remaining_percentage\":47,\"total_input_tokens\":92000,\"total_output_tokens\":64000},\"cost\":{\"total_duration_ms\":5640000}}' | $BIN
"

# Copy full as main screenshot
cp docs/screenshots/05-full.png screenshot.png

echo "Done! Screenshots saved to docs/screenshots/"
ls -la docs/screenshots/
