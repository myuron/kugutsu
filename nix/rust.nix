{
  lib,
  callPackage,
  makeRustPlatform,
  rust-bin,
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

  # PTY を使う統合テストは nix のビルドサンドボックスでは動かないことがあるため CI の cargo test に任せる
  doCheck = false;

  # wayland / vulkan / xkbcommon は dlopen されるので rpath に追加する
  postFixup = ''
    patchelf --add-rpath ${lib.makeLibraryPath gpuiDeps.runtimeLibs} $out/bin/kugutsu
  '';
}
