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
        # Native GUI stack for the opt-in webview (Tauri/wry, X1a), GTK4
        # (X1b), and Qt6/QML (X1c) shells. Kept out of the default shell
        # so `just check` stays light + hermetic (I10); only
        # `nix develop .#webview` pulls webkitgtk + gtk + Qt.
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
          # Qt6: qtbase (qmake + QtCore/Gui/Widgets) + qtdeclarative
          # (QtQuick/QML). The qtbase setup hook aggregates the module
          # include paths so qmetaobject's C++ glue finds QtQuick.
          qt6.qtbase
          qt6.qtdeclarative
        ];
        # Mesa's lavapipe: a Vulkan driver that runs on the CPU.
        # `gpuiLibs` carries the Vulkan *loader*, which is dispatch and
        # no rendering; a machine with no GPU (a headless server over
        # VNC, a CI runner, a VM without passthrough) carries no
        # *driver*, and gpui then dies in blade with
        # `NoSupportedDeviceFound`. Naming the manifest here lets the
        # shell open a window anywhere `nix develop` runs, with nothing
        # installed on the host — the preflight
        # (`closure-shell-gpui::gpui_preflight`) uses it only when the
        # machine has no driver of its own, since software rendering
        # costs an order of magnitude in frame time. Linux only: the
        # macOS path is Metal, and there is no ICD to name.
        softwareIcd =
          if pkgs.stdenv.hostPlatform.isLinux
          then "${pkgs.mesa}/share/vulkan/icd.d/lvp_icd.${pkgs.stdenv.hostPlatform.parsed.cpu.name}.json"
          else "";
      in {
        webview = pkgs.mkShell {
          packages = [
            rustToolchain.${system}
            pkgs.cargo-nextest
            pkgs.just
            pkgs.pkg-config
            pkgs.qt6.qtbase # qmake on PATH for qmetaobject's build script
          ];
          buildInputs = gpuiLibs ++ webviewLibs;
          LD_LIBRARY_PATH = pkgs.lib.makeLibraryPath (gpuiLibs ++ webviewLibs);
          CLOSURE_SOFTWARE_ICD = softwareIcd;
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
            # The CPU Vulkan driver the gpui shell falls back to when
            # the machine has none of its own. See `softwareIcd` above.
            CLOSURE_SOFTWARE_ICD = softwareIcd;
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
