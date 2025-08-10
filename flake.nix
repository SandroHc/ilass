{
  description = "ilass development environment";

  inputs = {
    nixpkgs.url = "github:NixOS/nixpkgs/nixos-unstable";
    flake-utils.url = "github:numtide/flake-utils";
    rust-overlay = {
      url = "github:oxalica/rust-overlay";
      inputs.nixpkgs.follows = "nixpkgs";
    };
  };

  outputs = { self, nixpkgs, flake-utils, rust-overlay }:
    flake-utils.lib.eachDefaultSystem (system:
      let
        overlays = [ (import rust-overlay) ];
        pkgs = import nixpkgs {
          inherit system overlays;
        };

        # Rust toolchain
        rustToolchain = pkgs.rust-bin.stable.latest.default.override {
          extensions = [ "rust-src" "rustfmt" "clippy" ];
        };

        # Platform-specific dependencies
        platformDeps = with pkgs; if stdenv.isDarwin then [
#          darwin.apple_sdk.frameworks.CoreFoundation
#          darwin.apple_sdk.frameworks.CoreVideo
#          darwin.apple_sdk.frameworks.CoreMedia
#          darwin.apple_sdk.frameworks.Security
#          darwin.apple_sdk.frameworks.VideoToolbox
#          darwin.apple_sdk.frameworks.AudioToolbox
#          libiconv
        ] else if stdenv.isLinux then [
#          alsa-lib
#          xorg.libX11
#          xorg.libXext
#          xorg.libXfixes
#          xorg.libxcb
#          xorg.libXcomposite
#          xorg.libXcursor
#          xorg.libXdamage
#          xorg.libXrandr
#          xorg.libXi
#          xorg.libXinerama
#          xorg.libXrender
#          libpulseaudio
#          libGL
#          vulkan-loader
#          wayland
        ] else [
          # Windows (cross-compilation support)
        ];

        # FFmpeg with full codec support
        ffmpeg-full = pkgs.ffmpeg-full.override {
          # Enable additional codecs and features
#          nonfreeLicensing = true;
#          fdkaacExtlib = true;
        };

        # Common build dependencies
        buildInputs = with pkgs; [
          # Core FFmpeg libraries
          ffmpeg-full
          ffmpeg-full.dev

          # Compression libraries
#          zlib
#          bzip2
#          xz

          # Build tools
          pkg-config
#          cmake
#          nasm
#          yasm

          # Additional codecs and libraries
#          x264
#          x265
#          libvpx
#          libaom
#          libopus
#          libvorbis
#          lame
#          fdk_aac
        ] ++ platformDeps;

        # Compile-time dependencies
        nativeBuildInputs = with pkgs; [
          rustToolchain
#          pkg-config
#          cmake
##          nasm
##          yasm
          clang
#          llvmPackages.bintools
        ];

        # Environment variables for linking
        shellHook = ''
          export LIBCLANG_PATH="${pkgs.libclang.lib}/lib"
#          export BINDGEN_EXTRA_CLANG_ARGS="-isystem ${pkgs.llvmPackages.clang-unwrapped.lib}/lib/clang/${pkgs.llvmPackages.clang.version}/include"

#          # FFmpeg library paths
#          export PKG_CONFIG_PATH="${ffmpeg-full.dev}/lib/pkgconfig:$PKG_CONFIG_PATH"
#          export LD_LIBRARY_PATH="${ffmpeg-full}/lib:$LD_LIBRARY_PATH"
#          export DYLD_LIBRARY_PATH="${ffmpeg-full}/lib:$DYLD_LIBRARY_PATH"

#          # For static linking
#          export FFMPEG_STATIC=0  # Set to 1 for static linking
#          export FFMPEG_PKG_CONFIG_PATH="${ffmpeg-full.dev}/lib/pkgconfig"

#          # Rust specific
#          export RUST_BACKTRACE=1
#          export RUSTFLAGS="-C link-arg=-Wl,-rpath,${ffmpeg-full}/lib"

		  echo ""
		  echo "ilass environment:"
          echo "  $(rustc --version)"
		  echo "  $(${ffmpeg-full}/bin/ffmpeg -version | head -n1)"
          echo ""
          echo "Available commands:"
          echo "  cargo build           - Build with dynamic linking"
          echo "  cargo build --features static - Build with static linking"
          echo "  ffmpeg -version       - Check FFmpeg installation"
          echo "  pkg-config --libs libavcodec - Check library linking"
          echo ""
        '';
      in
      {
        devShells.default = pkgs.mkShell {
          inherit buildInputs nativeBuildInputs shellHook;

          # Additional environment setup
          RUST_SRC_PATH = "${rustToolchain}/lib/rustlib/src/rust/library";
#          CC = "${pkgs.clang}/bin/clang";
#          CXX = "${pkgs.clang}/bin/clang++";
        };

#        # Package definition for the FFmpeg wrapper
#        packages.default = pkgs.rustPlatform.buildRustPackage {
#          pname = "ilass";
#          version = "0.1.0";
#          src = ./.;
#
#          cargoLock = {
#            lockFile = ./Cargo.lock;
#          };
#
#          inherit buildInputs nativeBuildInputs;
#
#          # Build configuration
#          buildFeatures = [ ]; # Default to dynamic linking
#
#          # Environment for building
#          PKG_CONFIG_PATH = "${ffmpeg-full.dev}/lib/pkgconfig";
#          LIBCLANG_PATH = "${pkgs.libclang.lib}/lib";
#
#          meta = with pkgs.lib; {
#            description = "Rust wrapper for FFmpeg with dynamic and static linking support";
#            license = licenses.mit; # TODO
#            maintainers = [ ];
#          };
#        };
#
#        # Static build variant
#        packages.static = pkgs.rustPlatform.buildRustPackage {
#          pname = "ilass-static";
#          version = "0.1.0";
#          src = ./.;
#
#          cargoLock = {
#            lockFile = ./Cargo.lock;
#          };
#
#          inherit nativeBuildInputs;
#
#          # For static builds, we need static libraries
#          buildInputs = with pkgs; [
#            ffmpeg-full.dev
#            zlib.static
#            bzip2.out
#            xz.out
#          ] ++ platformDeps;
#
#          buildFeatures = [ "static" ];
#
#          PKG_CONFIG_PATH = "${ffmpeg-full.dev}/lib/pkgconfig";
#          LIBCLANG_PATH = "${pkgs.libclang.lib}/lib";
#
#          meta = with pkgs.lib; {
#            description = "Rust wrapper for FFmpeg with static linking";
#            license = licenses.mit;
#            maintainers = [ ];
#          };
#        };
#
#        # Cross-compilation support for Windows
#        packages.windows = pkgs.pkgsCross.mingwW64.rustPlatform.buildRustPackage {
#          pname = "ilass-windows";
#          version = "0.1.0";
#          src = ./.;
#
#          cargoLock = {
#            lockFile = ./Cargo.lock;
#          };
#
#          nativeBuildInputs = with pkgs; [
#            rustToolchain
#            pkg-config
#            pkgsCross.mingwW64.stdenv.cc
#          ];
#
#          buildInputs = with pkgs.pkgsCross.mingwW64; [
#            windows.mingw_w64_pthreads
#          ];
#
#          # Windows-specific configuration would go here
#          # This is a basic setup - full Windows support would need more work
#
#          meta = with pkgs.lib; {
#            description = "Windows build of FFmpeg Rust wrapper";
#            license = licenses.mit;
#            platforms = [ "x86_64-windows" ];
#          };
#        };
      }
    );
}