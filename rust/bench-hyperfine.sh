#!/bin/bash
# Hyperfine benchmark for cc-statusline

cd "$(dirname "$0")"
INPUT='{"workspace":{"current_dir":"'"$(pwd)"'"}}'
echo "$INPUT" > /tmp/cc-bench-input.json

# Prime the cache
CC_STATUS_CACHE=1 ./target/release/cc-statusline < /tmp/cc-bench-input.json > /dev/null

echo "=== Hyperfine Benchmark ==="
hyperfine --warmup 5 -N \
  --input /tmp/cc-bench-input.json \
  -L mode default,minimal,fast,cache \
  'sh -c "[ {mode} = minimal ] && export CC_STATUS_MINIMAL=1; [ {mode} = fast ] && export CC_STATUS_FAST=1; [ {mode} = cache ] && export CC_STATUS_CACHE=1; ./target/release/cc-statusline"'
