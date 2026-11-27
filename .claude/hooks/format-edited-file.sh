#!/usr/bin/env bash
# PostToolUse(Edit|Write|MultiEdit) hook: 編集したファイルをその場で整形する。
# .rs は rustfmt、.nix は nix fmt（treefmt → nixfmt）。それ以外は何もしない。
# 構文エラーなどで整形できなかったときは exit 2 で stderr を Claude に返す。
set -euo pipefail

input=$(cat)
file=$(jq -r '.tool_input.file_path // empty' <<<"$input")
cwd=$(jq -r '.cwd // empty' <<<"$input")

case "$file" in
*.rs | *.nix) ;;
*) exit 0 ;;
esac
[[ -f "$file" ]] || exit 0

# Bash ツールと同じく direnv を通らないので、devShell / flake のあるリポジトリで実行する
root=$(git -C "${cwd:-.}" rev-parse --show-toplevel 2>/dev/null || echo "${cwd:-.}")
cd "$root"

case "$file" in
*.rs) cmd=(nix develop --command rustfmt --edition 2024 "$file") ;;
*.nix)
  # treefmt はリポジトリの外のファイルを扱えない
  [[ "$(realpath "$file")" == "$root"/* ]] || exit 0
  cmd=(nix fmt -- "$file")
  ;;
esac

if ! out=$("${cmd[@]}" 2>&1); then
  echo "$file を整形できませんでした:" >&2
  echo "$out" >&2
  exit 2
fi
