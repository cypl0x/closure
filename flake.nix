{
  description = "closure — local-first plain-text PKM kernel + shells";

  inputs = {
    nixpkgs.url = "github:NixOS/nixpkgs/nixos-unstable";
    systems.url = "github:nix-systems/default";
    fenix = {
      url = "github:nix-community/fenix";
      inputs.nixpkgs.follows = "nixpkgs";
    };
    treefmt-nix = {
      url = "github:numtide/treefmt-nix";
      inputs.nixpkgs.follows = "nixpkgs";
    };
  };

  outputs = {
    self,
    nixpkgs,
    systems,
    fenix,
    treefmt-nix,
  }: let
    eachSystem = f:
      nixpkgs.lib.genAttrs (import systems) (system:
        f {
          inherit system;
          pkgs = nixpkgs.legacyPackages.${system};
          fenixPkgs = fenix.packages.${system};
        });

    treefmtEval = eachSystem ({pkgs, ...}: treefmt-nix.lib.evalModule pkgs ./treefmt.nix);

    rustToolchain = eachSystem ({fenixPkgs, ...}:
      fenixPkgs.fromToolchainFile {
        file = ./rust-toolchain.toml;
        sha256 = "sha256-gh/xTkxKHL4eiRXzWv8KP7vfjSk61Iq48x47BEDFgfk=";
      });
  in {
    formatter = eachSystem ({system, ...}: treefmtEval.${system}.config.build.wrapper);

    devShells = eachSystem ({
        system,
        pkgs,
        ...
      }:
      # System libraries the optional `gpui` native shell needs to
      # link and run (Zed's GPU stack: Vulkan, Wayland/X11, xkbcommon,
      # fontconfig/freetype). Only required for
      # `cargo build -p closure-cli --features gpui`; the default
      # hermetic build never touches them.
      let
        gpuiLibs = with pkgs; [
          vulkan-loader
          libxkbcommon
          wayland
          xorg.libxcb
          xorg.libX11
          libxext
          fontconfig
          freetype
          libGL
        ];
        # Native GUI stack for the opt-in webview (Tauri/wry, X1a) and
        # GTK4 (X1b) shells. Kept out of the default shell so `just check`
        # stays light + hermetic (I10); only `nix develop .#webview`
        # pulls webkitgtk + gtk.
        webviewLibs = with pkgs; [
          gtk3
          gtk4
          glib
          cairo
          pango
          gdk-pixbuf
          atkmm
          webkitgtk_4_1
          libsoup_3
        ];
      in {
        webview = pkgs.mkShell {
          packages = [
            rustToolchain.${system}
            pkgs.cargo-nextest
            pkgs.just
            pkgs.pkg-config
          ];
          buildInputs = gpuiLibs ++ webviewLibs;
          LD_LIBRARY_PATH = pkgs.lib.makeLibraryPath (gpuiLibs ++ webviewLibs);
        };

        default = pkgs.mkShell {
          packages = [
            rustToolchain.${system}
            pkgs.cargo-nextest
            pkgs.cargo-fuzz
            pkgs.cargo-llvm-cov
            pkgs.just
            pkgs.ripgrep
            pkgs.fd
            pkgs.treefmt
            pkgs.alejandra
            pkgs.deadnix
            pkgs.taplo
            pkgs.prettier
            pkgs.pkg-config
          ];

          buildInputs = gpuiLibs;

          env = {
            RUST_BACKTRACE = "1";
          };

          # gpui dlopen's the Vulkan/Wayland/xkb libs at runtime.
          LD_LIBRARY_PATH = pkgs.lib.makeLibraryPath gpuiLibs;
        };
      });

    # `nix flake check` runs hermetic checks only: formatting + dead-nix
    # analysis. Cargo-based checks (test, clippy) require network for crate
    # fetch and run via `nix develop -c cargo ...` locally and in CI.
    checks = eachSystem ({
      system,
      pkgs,
      ...
    }: {
      # Only truly hermetic checks live here. The nix build sandbox has
      # no network, so cargo can't fetch crates.io deps inside a check
      # derivation — the cargo-based gates (clippy, nextest, coverage
      # threshold, wasm target, fuzz/replay) therefore run via
      # `nix develop -c just <recipe>` (see the justfile), where the
      # registry is reachable. `just check` is the one-command gate;
      # `just coverage` enforces the line-coverage floor.
      formatting = treefmtEval.${system}.config.build.check self;
      deadnix =
        pkgs.runCommand "deadnix-check" {
          nativeBuildInputs = [pkgs.deadnix];
        } ''
          cp -r ${self}/. .
          deadnix --fail .
          touch $out
        '';
    });
  };
}
