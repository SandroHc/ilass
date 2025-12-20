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

  outputs = {
    self,
    nixpkgs,
    flake-utils,
    rust-overlay,
  }:
    flake-utils.lib.eachDefaultSystem (
      system: let
        pkgs = import nixpkgs {
          inherit system;
          config = {allowUnfree = true;};
          overlays = [rust-overlay.overlays.default];
        };

        rustToolchain = pkgs.rust-bin.stable.latest.default.override {
          extensions = ["rust-src" "rustfmt" "clippy"];
        };

        ffmpeg-full = pkgs.ffmpeg-full.override {
          withUnfree = true;
        };
      in {
        devShells.default = pkgs.mkShell {
          nativeBuildInputs = with pkgs; [
            rustToolchain
            cargo-cross
            cargo-insta
            cargo-watch

            clang
            nasm
            pkg-config

            # project specific
            ffmpeg-full.dev
            lame # .mp3
            libvorbis # .vorbis
            opusfile # .opus
          ];

          buildInputs = with pkgs; [
          ];

          shellHook = ''
            export LIBCLANG_PATH="${pkgs.libclang.lib}/lib"

            echo ""
            echo "ilass environment:"
            echo "  Platform: ${system}"
            echo "  $(rustc --version)"
            echo "  ffmpeg $(${ffmpeg-full}/bin/ffmpeg -version | head -n1 | awk -F ' ' '{print $3}')"
            echo ""
            echo "Available commands:"
            echo "  cargo build"
            echo "  cargo build --features static,ffmpeg-next/build"
            echo "  pkg-config --libs libavcodec"
            echo ""
          '';

          RUST_SRC_PATH = "${rustToolchain}/lib/rustlib/src/rust/library";
        };
      }
    );
}
