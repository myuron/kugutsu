{
  lib,
  callPackage,
  makeRustPlatform,
  makeWrapper,
  rust-bin,
  wayland,
  vulkan-loader,
  libgit2,
  git,
}:
let
  toolchain = rust-bin.stable.latest.default;
  rustPlatform = makeRustPlatform {
    cargo = toolchain;
    rustc = toolchain;
  };
  gpuiDeps = callPackage ./gpui-deps.nix { };
in
rustPlatform.buildRustPackage {
  pname = "kugutsu";
  version = "0.1.0";
  src = ../.;
  cargoLock.lockFile = ../Cargo.lock;

  nativeBuildInputs = gpuiDeps.nativeBuildInputs ++ [ makeWrapper ];
  # gpui の test-support（dev-dependencies）が git2 経由で libgit2 を使う。checkPhase で必要
  buildInputs = gpuiDeps.buildInputs ++ [ libgit2 ];
  # プラグインマネージャーのテストはローカルの bare リポジトリに対して git を実行する
  nativeCheckInputs = [ git ];

  # wayland / vulkan は dlopen されるので rpath に追加する。
  # プラグインの取得に使う git は PATH の最後に足し、ユーザーの git と ~/.gitconfig を優先する
  postFixup = ''
    patchelf --add-rpath ${
      lib.makeLibraryPath [
        wayland
        vulkan-loader
      ]
    } $out/bin/kugutsu
    wrapProgram $out/bin/kugutsu --suffix PATH : ${lib.makeBinPath [ git ]}
  '';
}
