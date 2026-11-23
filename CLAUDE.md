# kugutsu

## Commit messages

コミットメッセージは [Conventional Commits](https://www.conventionalcommits.org/ja/v1.0.0/) に準拠すること。

```
<type>[optional scope]: <description>

[optional body]

[optional footer(s)]
```

- type: `feat` / `fix` / `docs` / `style` / `refactor` / `perf` / `test` / `build` / `ci` / `chore` / `revert`
- 破壊的変更は type の後に `!` を付けるか、footer に `BREAKING CHANGE:` を書く
- 例: `feat(cli): add --verbose option`, `ci: add cargo clippy step`
