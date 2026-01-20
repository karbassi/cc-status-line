#!/bin/bash
# Test script for cc-statusline

INPUT='{"workspace":{"current_dir":"'"$(pwd)"'"}}'

echo "=== Default mode ==="
echo "$INPUT" | ./target/release/cc-statusline

echo ""
echo "=== Minimal mode ==="
echo "$INPUT" | CC_STATUS_MINIMAL=1 ./target/release/cc-statusline

echo ""
echo "=== Fast mode ==="
echo "$INPUT" | CC_STATUS_FAST=1 ./target/release/cc-statusline

echo ""
echo "=== Cache mode (first run) ==="
echo "$INPUT" | CC_STATUS_CACHE=1 ./target/release/cc-statusline

echo ""
echo "=== Cache mode (second run - should be cached) ==="
echo "$INPUT" | CC_STATUS_CACHE=1 ./target/release/cc-statusline
