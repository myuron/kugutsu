{
  inputs = {
    nixpkgs.url = "github:nixos/nixpkgs/nixpkgs-unstable";
    flake-parts.url = "github:hercules-ci/flake-parts";
    treefmt-nix.url = "github:numtide/treefmt-nix";
    rust-overlay.url = "github:oxalica/rust-overlay";
    rust-overlay.inputs.nixpkgs.follows = "nixpkgs";
    # home-manager モジュールのテストにだけ使う
    home-manager.url = "github:nix-community/home-manager";
    home-manager.inputs.nixpkgs.follows = "nixpkgs";
  };

  outputs =
    inputs@{
      self,
      nixpkgs,
      flake-parts,
      treefmt-nix,
      rust-overlay,
      home-manager,
    }:
    flake-parts.lib.mkFlake { inherit inputs; } {
      imports = [
        treefmt-nix.flakeModule
      ];
      systems = [
        "x86_64-linux"
      ];
      flake = {
        homeManagerModules.default = import ./nix/home-manager.nix { inherit self; };
      };
      perSystem = { pkgs, system, ... }: {
        _module.args.pkgs = import nixpkgs {
          inherit system;
          overlays = [
            rust-overlay.overlays.default
          ];
        };
        treefmt = {
          projectRootFile = "flake.nix";
          programs = {
            nixfmt.enable = true;
          };
        };
        packages = {
          default = pkgs.callPackage ./nix/rust.nix { };
        };
        checks = {
          home-manager-module = pkgs.callPackage ./nix/home-manager-test.nix {
            inherit home-manager;
            module = self.homeManagerModules.default;
          };
        };
        devShells = {
          default =
            let
              gpuiDeps = pkgs.callPackage ./nix/gpui-deps.nix { };
            in
            pkgs.mkShell {
              packages = [
                pkgs.rust-bin.stable.latest.default
                pkgs.git
              ]
              ++ gpuiDeps.nativeBuildInputs;
              buildInputs = gpuiDeps.buildInputs;
              LD_LIBRARY_PATH = pkgs.lib.makeLibraryPath gpuiDeps.runtimeLibs;
              # 開発中の `cargo run` ではリポジトリのサンプル設定を読む
              shellHook = ''
                export KUGUTSU_CONFIG="$PWD/examples/config/kugutsu/init.lua"
              '';
            };
        };
      };
    };
}
