# home-manager モジュールのテスト。設定を評価して、生成されるファイルとリンクを検査する。
{
  lib,
  pkgs,
  runCommand,
  writeShellScriptBin,
  home-manager,
  module,
}:
let
  # 本体のビルドは重いので、テストではスタブを使う
  stub = writeShellScriptBin "kugutsu" "";

  mkPlugin =
    name:
    runCommand name { } ''
      mkdir -p $out/colors
      echo 'return {}' > $out/colors/${name}.lua
    '';
  one = mkPlugin "one.kugutsu";
  twoSrc = mkPlugin "source";

  hm =
    kugutsu:
    home-manager.lib.homeManagerConfiguration {
      inherit pkgs;
      modules = [
        module
        {
          home.username = "test";
          home.homeDirectory = "/home/test";
          home.stateVersion = "25.05";
          programs.kugutsu = kugutsu;
        }
      ];
    };

  enabled = hm {
    enable = true;
    package = stub;
    plugins = [
      one
      {
        name = "two.kugutsu";
        src = twoSrc;
      }
    ];
    initLua = ''kugutsu.plugins.setup({ "a/one.kugutsu", "b/two.kugutsu" })'';
  };

  # home-manager は失敗した assertion を評価時に throw する
  evaluates = configuration: (builtins.tryEval configuration.activationPackage.drvPath).success;

  duplicate = hm {
    enable = true;
    package = stub;
    plugins = [
      one
      {
        name = "one.kugutsu";
        src = twoSrc;
      }
    ];
  };

  disabled = hm {
    enable = false;
    package = stub;
    plugins = [ one ];
    initLua = "-- unused";
  };

  evalChecks = [
    {
      name = "duplicate plugin names fail an assertion";
      ok = !evaluates duplicate;
    }
    {
      name = "the enabled configuration evaluates";
      ok = evaluates enabled;
    }
    {
      name = "a disabled module links no plugins";
      ok = !(disabled.config.xdg.dataFile ? "kugutsu/plugins/one.kugutsu");
    }
    {
      name = "a disabled module writes no init.lua";
      ok = !(disabled.config.xdg.configFile ? "kugutsu/init.lua");
    }
    {
      name = "a disabled module installs no package";
      ok = !(lib.elem stub disabled.config.home.packages);
    }
  ];
  failedEvalChecks = map (c: c.name) (lib.filter (c: !c.ok) evalChecks);

  files = "${enabled.activationPackage}/home-files";
in
assert lib.assertMsg (failedEvalChecks == [ ]) (
  "home-manager module checks failed: " + lib.concatStringsSep ", " failedEvalChecks
);
runCommand "kugutsu-home-manager-module-test" { } ''
  set -eu
  fail() { echo "FAIL: $*" >&2; exit 1; }

  plugins=${files}/.local/share/kugutsu/plugins
  [ -L "$plugins/one.kugutsu" ] || fail "one.kugutsu is not a symlink"
  [ "$(readlink -f "$plugins/one.kugutsu")" = "${one}" ] || fail "one.kugutsu does not point to its derivation"
  [ -L "$plugins/two.kugutsu" ] || fail "two.kugutsu is not a symlink"
  [ "$(readlink -f "$plugins/two.kugutsu")" = "${twoSrc}" ] || fail "two.kugutsu does not point to its src"

  init=${files}/.config/kugutsu/init.lua
  [ -f "$init" ] || fail "init.lua is missing"
  grep -qF 'kugutsu.plugins.setup({ "a/one.kugutsu", "b/two.kugutsu" })' "$init" \
    || fail "init.lua has unexpected content: $(cat "$init")"

  [ -x ${enabled.activationPackage}/home-path/bin/kugutsu ] || fail "kugutsu is not installed"

  touch $out
''
