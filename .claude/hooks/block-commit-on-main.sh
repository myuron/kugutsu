#!/usr/bin/env bash
# PreToolUse(Bash) hook: main ブランチ上での `git commit` を拒否する。
# exit 2 で終了すると Claude Code はコマンドを実行せず、stderr の内容を Claude に返す。
set -euo pipefail

input=$(cat)
command=$(jq -r '.tool_input.command // empty' <<<"$input")
cwd=$(jq -r '.cwd // empty' <<<"$input")

# `git commit` / `git -C dir commit` などを含まなければ対象外
git_commit='(^|[;&|(`[:space:]])git([[:space:]]+-[^[:space:]]+([[:space:]]+[^-[:space:]][^[:space:]]*)?)*[[:space:]]+commit([[:space:]]|$)'
if ! grep -Eq "$git_commit" <<<"$command"; then
  exit 0
fi

# 同じコマンドの中でブランチを切ってから commit する場合は許可する
new_branch='git[[:space:]]+(switch[[:space:]]+(-c|-C|--create)|checkout[[:space:]]+(-b|-B))[[:space:]]'
if grep -Eq "$new_branch" <<<"$command"; then
  exit 0
fi

branch=$(git -C "${cwd:-.}" branch --show-current 2>/dev/null || true)
if [[ "$branch" == "main" ]]; then
  echo "main ブランチ上での git commit は禁止されています。先に 'git switch -c <branch>' で作業ブランチを切ってから commit してください。" >&2
  exit 2
fi
