# Kugutsu の home-manager モジュール。
#
# プラグインは Nix store へのリンクとして `$XDG_DATA_HOME/kugutsu/plugins/<name>` に置く。
# プラグインマネージャーは symlink を external として扱うので、`kugutsu install` はそれを
# clone・checkout・削除せず、spec に名前があれば読み込む。
{ self }:
{
  config,
  lib,
  pkgs,
  ...
}:
let
  cfg = config.programs.kugutsu;

  pluginModule = lib.types.submodule {
    options = {
      name = lib.mkOption {
        type = lib.types.str;
        example = "catppuccin.kugutsu";
        description = "Name of the plugin in the spec: the last part of its URL without `.git`, or its `name`.";
      };
      src = lib.mkOption {
        type = lib.types.path;
        description = "Directory of the plugin.";
      };
    };
  };

  # derivation はその名前（`pname`、無ければ `name`）を使う
  normalize =
    plugin:
    if lib.isDerivation plugin then
      {
        name = lib.getName plugin;
        src = plugin;
      }
    else
      plugin;

  plugins = map normalize cfg.plugins;
  names = map (plugin: plugin.name) plugins;
  duplicates = lib.unique (lib.filter (name: lib.count (n: n == name) names > 1) names);
in
{
  options.programs.kugutsu = {
    enable = lib.mkEnableOption "Kugutsu";

    package = lib.mkOption {
      type = lib.types.package;
      default = self.packages.${pkgs.stdenv.hostPlatform.system}.default;
      defaultText = lib.literalExpression "kugutsu.packages.\${pkgs.stdenv.hostPlatform.system}.default";
      description = "The Kugutsu package.";
    };

    plugins = lib.mkOption {
      type = lib.types.listOf (lib.types.either lib.types.package pluginModule);
      default = [ ];
      example = lib.literalExpression ''
        [
          (pkgs.fetchFromGitHub {
            name = "catppuccin.kugutsu";
            owner = "myuron";
            repo = "catppuccin.kugutsu";
            rev = "v1.0.0";
            hash = "...";
          })
          { name = "notes.kugutsu"; src = ./notes; }
        ]
      '';
      description = ''
        Plugins linked to `$XDG_DATA_HOME/kugutsu/plugins/<name>`. Each one is a derivation, whose
        name is the plugin name, or `{ name, src }`. The name must match the plugin name in the
        spec of `init.lua`; they are loaded only when they are in the spec.
      '';
    };

    initLua = lib.mkOption {
      type = lib.types.nullOr lib.types.lines;
      default = null;
      description = "Contents of `$XDG_CONFIG_HOME/kugutsu/init.lua`. When `null`, the file is not managed.";
    };
  };

  config = lib.mkIf cfg.enable {
    assertions = [
      {
        assertion = duplicates == [ ];
        message = "programs.kugutsu.plugins: duplicate plugin names: ${lib.concatStringsSep ", " duplicates}";
      }
    ];

    home.packages = [ cfg.package ];

    xdg.dataFile = lib.listToAttrs (
      map (plugin: lib.nameValuePair "kugutsu/plugins/${plugin.name}" { source = plugin.src; }) plugins
    );

    xdg.configFile."kugutsu/init.lua" = lib.mkIf (cfg.initLua != null) { text = cfg.initLua; };
  };
}
