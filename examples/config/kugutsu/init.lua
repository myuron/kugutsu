-- Kugutsu の動作確認用の設定。
-- devShell（nix develop / direnv）では KUGUTSU_CONFIG でこのファイルが読まれる: nix develop --command cargo run

-- prefix を Ctrl-a にする（Ctrl-b は端末にそのまま送られるようになる）
kugutsu.opt.prefix = "ctrl-a"

-- フォント（インストールされていなければ既定の等幅フォントになる）
kugutsu.opt.font_family = "JetBrains Mono"

-- 既定より大きくして違いを分かりやすくする
kugutsu.opt.font_size = 18
kugutsu.opt.line_height = 1.5

-- 現在の値を読み出すこともできる
assert(kugutsu.opt.font_size == 18)

-- エラー表示を確かめるときは、次のどれかのコメントを外す。
-- ステータスラインの右に赤字でエラーが出て、ここより上の設定はそのまま効く。
-- kugutsu.opt.foo = 1                -- 存在しないオプション
-- kugutsu.opt.font_size = "big"      -- 型が違う
-- kugutsu.opt.line_height = 0        -- 0 以下
-- kugutsu.opt.prefix = "ctrl-"       -- キーとして解釈できない
-- error("boom")                      -- 実行時エラー
