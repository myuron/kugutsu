{
  lib,
  callPackage,
  makeRustPlatform,
  rust-bin,
  wayland,
  vulkan-loader,
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

  inherit (gpuiDeps) nativeBuildInputs buildInputs;

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
