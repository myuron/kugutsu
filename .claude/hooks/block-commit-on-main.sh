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

# commit するリポジトリ。`git -C <dir>` があればそのディレクトリ、無ければコマンド中の
# 最後の `cd <dir>`、どちらも無ければセッションの cwd。git worktree で作業ブランチの
# ディレクトリへ移って commit する場合も、そのディレクトリのブランチで判定する
last_arg() {
  grep -oE "$1[[:space:]]+[^;&|()[:space:]]+" <<<"$command" | tail -n 1 |
    sed -E "s/^$1[[:space:]]+//; s/^[\"']//; s/[\"']$//" || true
}
target=$(last_arg 'git[[:space:]]+-C')
if [[ -z "$target" ]]; then
  target=$(last_arg '(^|[;&|(`[:space:]])cd')
fi
target=${target/#\~/$HOME}

branch=$(cd "${cwd:-.}" && git -C "${target:-.}" branch --show-current 2>/dev/null || true)
if [[ "$branch" == "main" ]]; then
  echo "main ブランチ上での git commit は禁止されています。先に 'git switch -c <branch>' で作業ブランチを切ってから commit してください。" >&2
  exit 2
fi
