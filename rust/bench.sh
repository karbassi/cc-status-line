#!/bin/bash
# Benchmark script for cc-statusline

cd "$(dirname "$0")"
INPUT='{"workspace":{"current_dir":"'"$(pwd)"'"}}'
RUNS=50

echo "=== Benchmarking $RUNS runs each ==="
echo ""

echo "Default mode (full diff):"
time (for i in $(seq 1 $RUNS); do echo "$INPUT" | ./target/release/cc-statusline > /dev/null; done)
echo ""

echo "Minimal mode (branch only):"
time (for i in $(seq 1 $RUNS); do echo "$INPUT" | CC_STATUS_MINIMAL=1 ./target/release/cc-statusline > /dev/null; done)
echo ""

echo "Fast mode (status-based):"
time (for i in $(seq 1 $RUNS); do echo "$INPUT" | CC_STATUS_FAST=1 ./target/release/cc-statusline > /dev/null; done)
echo ""

# Prime the cache
echo "$INPUT" | CC_STATUS_CACHE=1 ./target/release/cc-statusline > /dev/null

echo "Cache mode (mmap - cached):"
time (for i in $(seq 1 $RUNS); do echo "$INPUT" | CC_STATUS_CACHE=1 ./target/release/cc-statusline > /dev/null; done)
