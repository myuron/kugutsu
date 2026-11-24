# gpui (Linux: wayland / x11 / blade(vulkan)) のビルド・実行に必要なシステム依存
{
  pkg-config,
  fontconfig,
  freetype,
  libxkbcommon,
  wayland,
  vulkan-loader,
  libxcb,
  libx11,
  libxext,
  libxrandr,
  libxi,
  alsa-lib,
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
    libx11
    libxext
    libxrandr
    libxi
    alsa-lib
  ];
  # dlopen されるため実行時に見える必要があるライブラリ
  runtimeLibs = [
    wayland
    libxkbcommon
    vulkan-loader
    libx11
    libxcb
    libxext
    libxrandr
    libxi
  ];
}
