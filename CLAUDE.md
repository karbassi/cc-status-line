# Claude Code Instructions

This file provides guidance to Claude Code (claude.ai/code) when working with this repository.

## Project

Lightweight, fast status line for Claude Code CLI. Bash prototype first, then Rust rewrite.

## Git

Commit after each step. Short messages. Run in background.

Never use `git add -A` or `git add .`. Always add specific files relevant to the current task. Multiple Claude sessions may run concurrently.

## Workflow

Use subagents (Task tool) as much as possible. Run in background when possible.
