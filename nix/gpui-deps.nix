# gpui のビルド・実行に必要なシステム依存
# - Linux: wayland / x11 / blade(vulkan)
# - macOS: Metal と Apple SDK のフレームワーク。gpui（と gpui_media）の build.rs が bindgen を使う
{
  stdenv,
  pkg-config,
  rustPlatform,
  fontconfig,
  freetype,
  libxkbcommon,
  wayland,
  vulkan-loader,
  libxcb,
  apple-sdk_15,
}:
if stdenv.hostPlatform.isDarwin then
  {
    nativeBuildInputs = [
      pkg-config
      rustPlatform.bindgenHook
    ];
    buildInputs = [ apple-sdk_15 ];
    runtimeLibs = [ ];
  }
else
  rec {
    nativeBuildInputs = [ pkg-config ];
    buildInputs = [
      fontconfig
      freetype
      libxkbcommon
      wayland
      vulkan-loader
      libxcb
    ];
    # 実行時に見える必要があるライブラリ（wayland / vulkan は dlopen される）
    runtimeLibs = [
      wayland
      libxkbcommon
      vulkan-loader
      libxcb
    ];
  }
