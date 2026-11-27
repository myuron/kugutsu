//! プラグインマネージャー。
//!
//! プラグインは `init.lua` の `kugutsu.plugins.setup` で宣言し（[`spec`]）、`kugutsu.lock` で
//! コミットを固定し（`kugutsu.lock`）、`$XDG_DATA_HOME/kugutsu/plugins/<name>/` に置く（[`paths`]）。
//!
//! Nix flake と同じく、GUI の起動はディスクを読むだけ（[`loader`]）。lockfile と plugins
//! ディレクトリを変えるのは CLI の `kugutsu lock` / `update` / `install` だけ（[`fetcher`]）。

pub mod loader;
pub mod paths;
pub mod spec;
