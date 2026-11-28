{
  lib,
  stdenv,
  callPackage,
  makeRustPlatform,
  makeWrapper,
  rust-bin,
  wayland,
  vulkan-loader,
  libgit2,
  git,
  imagemagick,
  desktop-file-utils,
  libicns,
}:
let
  inherit (stdenv.hostPlatform) isLinux isDarwin;
  inherit ((lib.importTOML ../Cargo.toml).package) version;
  toolchain = rust-bin.stable.latest.default;
  rustPlatform = makeRustPlatform {
    cargo = toolchain;
    rustc = toolchain;
  };
  gpuiDeps = callPackage ./gpui-deps.nix { };
  app = "$out/Applications/Kugutsu.app/Contents";
  # プラグインの取得に使う git は PATH の最後に足し、ユーザーの git と ~/.gitconfig を優先する
  gitPath = "--suffix PATH : ${lib.makeBinPath [ git ]}";
in
rustPlatform.buildRustPackage {
  pname = "kugutsu";
  inherit version;
  src = ../.;
  cargoLock.lockFile = ../Cargo.lock;

  nativeBuildInputs =
    gpuiDeps.nativeBuildInputs
    ++ [
      makeWrapper
      imagemagick
    ]
    ++ lib.optionals isDarwin [ libicns ];
  # gpui の test-support（dev-dependencies）が git2 経由で libgit2 を使う。checkPhase で必要
  buildInputs = gpuiDeps.buildInputs ++ [ libgit2 ];
  # プラグインマネージャーのテストはローカルの bare リポジトリに対して git を実行する
  nativeCheckInputs = [ git ];

  # ランチャー向けのデスクトップエントリとアイコン。元の PNG（1254x1254）は hicolor の
  # 標準サイズではないので、ビルド時に縮小して置く
  postInstall = lib.optionalString isLinux ''
    install -Dm644 assets/kugutsu.desktop $out/share/applications/kugutsu.desktop
    for size in 256 512; do
      dir=$out/share/icons/hicolor/''${size}x''${size}/apps
      mkdir -p $dir
      magick assets/kugutsu.png -resize ''${size}x''${size} $dir/kugutsu.png
    done
  '';

  doInstallCheck = true;
  nativeInstallCheckInputs = lib.optionals isLinux [ desktop-file-utils ];
  installCheckPhase = ''
    runHook preInstallCheck
  ''
  + lib.optionalString isLinux ''
    desktop-file-validate $out/share/applications/kugutsu.desktop
  ''
  + lib.optionalString isDarwin ''
    for file in Info.plist MacOS/kugutsu MacOS/.kugutsu-wrapped Resources/kugutsu.icns; do
      [ -s ${app}/$file ] || { echo "Kugutsu.app is missing $file" >&2; exit 1; }
    done
    grep -q '<string>${version}</string>' ${app}/Info.plist
  ''
  + ''
    runHook postInstallCheck
  '';

  # wayland / vulkan は dlopen されるので rpath に追加する（Linux のみ）
  postFixup =
    lib.optionalString isLinux ''
      patchelf --add-rpath ${
        lib.makeLibraryPath [
          wayland
          vulkan-loader
        ]
      } $out/bin/kugutsu
    ''
    # macOS の .app バンドル。Dock のアイコンなどは実行中のバイナリがあるバンドルから決まるので、
    # バイナリをバンドルの中にコピーする（bin への symlink では Kugutsu.app と見なされない）
    + lib.optionalString isDarwin ''
      install -Dm755 $out/bin/kugutsu ${app}/MacOS/.kugutsu-wrapped
      makeWrapper ${app}/MacOS/.kugutsu-wrapped ${app}/MacOS/kugutsu ${gitPath}
      substitute assets/Info.plist ${app}/Info.plist --subst-var version

      iconset=$(mktemp -d)
      for size in 16 32 128 256 512 1024; do
        magick assets/kugutsu.png -resize ''${size}x''${size} $iconset/$size.png
      done
      mkdir -p ${app}/Resources
      png2icns ${app}/Resources/kugutsu.icns $iconset/*.png
    ''
    + ''
      wrapProgram $out/bin/kugutsu ${gitPath}
    '';
}
