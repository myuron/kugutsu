{
  lib,
  callPackage,
  makeRustPlatform,
  rust-bin,
  wayland,
  vulkan-loader,
  libgit2,
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

  inherit (gpuiDeps) nativeBuildInputs;
  # gpui の test-support（dev-dependencies）が git2 経由で libgit2 を使う。checkPhase で必要
  buildInputs = gpuiDeps.buildInputs ++ [ libgit2 ];

  # wayland / vulkan は dlopen されるので rpath に追加する
  postFixup = ''
    patchelf --add-rpath ${
      lib.makeLibraryPath [
        wayland
        vulkan-loader
      ]
    } $out/bin/kugutsu
  '';
}
