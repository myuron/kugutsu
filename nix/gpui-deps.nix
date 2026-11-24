# gpui (Linux: wayland / x11 / blade(vulkan)) のビルド・実行に必要なシステム依存
{
  pkg-config,
  fontconfig,
  freetype,
  libxkbcommon,
  wayland,
  vulkan-loader,
  libxcb,
}:
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
