#!/usr/bin/env bash
# Stop hook: 作業を終える前に CI と同じ clippy と test を通す。
# Rust 関係のファイルに変更があるときだけ走らせ、警告やエラー、テストの失敗があれば
# exit 2 で出力を Claude に返して修正させる。
set -euo pipefail

input=$(cat)
cwd=$(jq -r '.cwd // empty' <<<"$input")

# この hook が返したエラーを直している途中の Stop では走らせない（無限ループ防止）
if [[ "$(jq -r '.stop_hook_active // false' <<<"$input")" == "true" ]]; then
  exit 0
fi

root=$(git -C "${cwd:-.}" rev-parse --show-toplevel 2>/dev/null) || exit 0
cd "$root"

if ! git status --porcelain | grep -Eq '\.rs$|Cargo\.(toml|lock)$'; then
  exit 0
fi

if ! out=$(nix develop --command cargo clippy --all-targets --locked --quiet -- -D warnings 2>&1); then
  echo "cargo clippy --all-targets --locked -- -D warnings が失敗しました。修正してください:" >&2
  tail -n 80 <<<"$out" >&2
  exit 2
fi

if ! out=$(nix develop --command cargo test --all-targets --locked --quiet 2>&1); then
  echo "cargo test --all-targets --locked が失敗しました。修正してください:" >&2
  tail -n 80 <<<"$out" >&2
  exit 2
fi
