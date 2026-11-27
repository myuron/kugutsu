# CLAUDE.md

## 概要

- このプロジェクトはADE(Agentic Development Environment)を作成する

## 採用技術

- プログラミング言語: Rust
- 描画エンジン: gpui

## 開発環境

- ビルドに必要なツールとライブラリ（Rust ツールチェイン、wayland・vulkan など gpui の依存）は Nix の devShell（`flake.nix`）で用意している
- 普段は direnv（`.envrc`）が devShell を読み込むが、Claude Code の Bash ツールは direnv を通らない。そのため `cargo` などのビルド・テスト・lint は必ず `nix develop --command` を付けて実行する
  - 例: `nix develop --command cargo test --all-targets`
  - 付けずに実行するとリンクエラーになる
- CI と同じチェック
  - `nix develop --command cargo fmt --all --check`
  - `nix develop --command cargo clippy --all-targets --locked -- -D warnings`
  - `nix develop --command cargo test --all-targets --locked`
  - `nix flake check`（treefmt を含む）と `nix build`（サンドボックス内でテストも実行する）
- `python3` は入っていない。ファイル編集にスクリプトを使わず、エディタのツールを使う

## 作業について

- テスト駆動開発で行うこと。次の順番を守る
  1. テストを書く
  2. テストを実行し、期待どおりに失敗することを確認する（コンパイルエラーや `todo!()` による失敗でもよい）
  3. テストが通る最小限の実装をする
  4. テストを実行し、通ることを確認する
  5. 必要ならリファクタリングし、再びテストが通ることを確認する
- gpui のビュー（キー操作・クリック・表示の切り替えなど）も `gpui::TestAppContext` / `VisualTestContext` でテストする。「gpui に依存するからテストできない」として省かないこと

## ブランチと作業ディレクトリ

- 作業は main で直接行わず、必ず git worktree で作業ブランチごとのディレクトリを作って行う
  - 例: `git worktree add ../kugutsu-<branch> -b <branch>`
- 作業が終わってマージしたら `git worktree remove` で片付ける

## コミット規約

- Conventional Commitsに準拠すること
- コミットメッセージは件名・本文ともに英語で書くこと
