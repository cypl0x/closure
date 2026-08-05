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

    # The libraries gpui needs to link against and to find at run time.
    # Named once: the devShell puts them on `LD_LIBRARY_PATH` and the
    # packaged binary is wrapped with them, and those two lists
    # disagreeing is how a build that works in the shell fails as an
    # artefact.
    gpuiBuildInputs = pkgs:
      with pkgs; [
        vulkan-loader
        libxkbcommon
        wayland
        libxcb
        libx11
        libxext
        fontconfig
        freetype
        libGL
      ];

    rustToolchain = eachSystem ({fenixPkgs, ...}:
      fenixPkgs.fromToolchainFile {
        file = ./rust-toolchain.toml;
        sha256 = "sha256-gh/xTkxKHL4eiRXzWv8KP7vfjSk61Iq48x47BEDFgfk=";
      });
  in {
    formatter = eachSystem ({system, ...}: treefmtEval.${system}.config.build.wrapper);

    # A closure you can `nix build` and run, rather than a devShell you
    # have to be inside: "Currently we don't have any outputs or build
    # steps that create like a self contained app."
    #
    # Crates are vendored from `Cargo.lock` by `buildRustPackage`, so
    # the build is hermetic — which is also what lets the cargo gates
    # below run as real flake checks. Without a lockfile-driven vendor
    # they cannot, because the sandbox has no network; that was the
    # reason `nix flake check` had only formatting in it.
    packages = eachSystem ({
      system,
      pkgs,
      ...
    }: let
      rust = rustToolchain.${system};
      platform = pkgs.makeRustPlatform {
        cargo = rust;
        rustc = rust;
      };
      # The lavapipe manifest, named for the packaged build the same
      # way the devShell names it. Without it, `nix build .#gpui`
      # produced a binary that refused to open a window on any machine
      # with no GPU driver — so "a closure you can `nix build` and run"
      # was still untrue of the one case that most needs it. The
      # preflight reaches for this only when the machine has no driver
      # of its own.
      softwareIcdPkg =
        if pkgs.stdenv.hostPlatform.isLinux
        then "${pkgs.mesa}/share/vulkan/icd.d/lvp_icd.${pkgs.stdenv.hostPlatform.parsed.cpu.name}.json"
        else "";
      # The parts every variant shares. `buildRustPackage` needs a
      # single source of truth for these or the two packages drift.
      common = {
        pname = "closure";
        version = "0.0.0";
        src = self;
        cargoLock = {
          lockFile = ./Cargo.lock;
          allowBuiltinFetchGit = true;
        };
        # The workspace has a crate per shell; only the CLI produces a
        # binary anyone runs.
        cargoBuildFlags = ["-p" "closure-cli"];
        doCheck = false;
        # A nix source has no `.git`, so the build script cannot find
        # the revision and honestly reports "unknown commit". The flake
        # knows it, though, so it hands it over — and `dirtyShortRev`
        # is what a build from an uncommitted tree gets, which keeps
        # the claim as honest as the git path does.
        CLOSURE_GIT_COMMIT = self.shortRev or self.dirtyShortRev or null;
        meta = {
          description = "local-first plain-text PKM kernel";
          mainProgram = "closure";
        };
      };
      # closure shells out to `git` for the status fringes, the line
      # diffs and `closure sync`. Left to `$PATH` that is a runtime
      # dependency the package does not declare — it works on the
      # machine that built it and silently does nothing on a machine
      # without git, because a failed `git` reads the same as "not a
      # repository". Prefixing rather than setting so a user who wants
      # their own git still gets it.
      wrapGit = ''
        wrapProgram $out/bin/closure \
          --prefix PATH : ${pkgs.git}/bin
      '';
    in {
      default = platform.buildRustPackage (common
        // {
          nativeBuildInputs = [pkgs.makeWrapper];
          postInstall = wrapGit;
        });

      # The windowed build, self-contained: gpui needs its libraries
      # found at *run* time, not only at link time, so the binary is
      # wrapped with them rather than left to a devShell's
      # LD_LIBRARY_PATH. That is the difference between an executable
      # and an executable you can copy somewhere.
      gpui = platform.buildRustPackage (common
        // {
          pname = "closure-gpui";
          buildFeatures = ["gpui"];
          nativeBuildInputs = [pkgs.pkg-config pkgs.makeWrapper];
          buildInputs = gpuiBuildInputs pkgs;
          # The bundled font, resolved at build time exactly as the
          # devShell does it.
          CLOSURE_FONT_DIR = "${pkgs.maple-mono.NF}/share/fonts/truetype";
          postInstall = ''
            wrapProgram $out/bin/closure \
              --prefix LD_LIBRARY_PATH : ${pkgs.lib.makeLibraryPath (gpuiBuildInputs pkgs)} \
              --prefix PATH : ${pkgs.git}/bin \
              --set-default CLOSURE_SOFTWARE_ICD ${softwareIcdPkg}

            # Desktop integration. gpui takes no window-icon option, so
            # the icon reaches Alt-Tab the way every other X11/Wayland
            # app's does: the window reports `app_id`
            # (`net.wolfhard.closure`, set in WindowOptions), the WM
            # matches it against `StartupWMClass` in this file, and
            # draws the `Icon=` it names.
            install -Dm644 ${./assets/net.wolfhard.closure.desktop} \
              $out/share/applications/net.wolfhard.closure.desktop
            for size in 16 24 32 48 64 128 256 512; do
              install -Dm644 ${./assets}/icons/''${size}x''${size}/closure.png \
                $out/share/icons/hicolor/''${size}x''${size}/apps/net.wolfhard.closure.png
            done
            install -Dm644 ${./assets/closure.svg} \
              $out/share/icons/hicolor/scalable/apps/net.wolfhard.closure.svg
          '';
        });
    });

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

        # The faces the gpui shell embeds. Maple Mono NF is OFL-1.1, so
        # redistribution is allowed; the bytes come from here rather
        # than from git because a face is two and a half megabytes and
        # five of them do not belong in a source history. The lockfile
        # pins them like every other input.
        #
        # Without it the build embeds nothing and the shell falls back
        # to the system font stack — which is what it did before, and
        # is why `*bold*` was not bold.
        fontDir = "${pkgs.maple-mono.NF}/share/fonts/truetype";
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
          CLOSURE_FONT_DIR = fontDir;
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
            # See `fontDir` above: what the gpui shell embeds.
            CLOSURE_FONT_DIR = fontDir;
          };

          # gpui dlopen's the Vulkan/Wayland/xkb libs at runtime.
          LD_LIBRARY_PATH = pkgs.lib.makeLibraryPath gpuiLibs;
        };
      });

    # `nix flake check` runs the real gates: "Include all of the tests,
    # linter etc. Even add more linters like deadnix, alejandra etc.
    # Clippy has to be run from there too."
    #
    # It used to be formatting and dead-nix alone, on the reasoning that
    # the sandbox has no network so cargo cannot fetch crates. True of
    # a bare `cargo build`; not true once the lockfile is vendored,
    # which `packages` above now does. The cargo gates are derivations
    # built from that same vendored source, so they are as hermetic as
    # anything else here.
    checks = eachSystem ({
      system,
      pkgs,
      ...
    }: let
      rust = rustToolchain.${system};
      platform = pkgs.makeRustPlatform {
        cargo = rust;
        rustc = rust;
      };
      cargoGate = name: args:
        platform.buildRustPackage {
          pname = "closure-${name}";
          version = "0.0.0";
          src = self;
          cargoLock = {
            lockFile = ./Cargo.lock;
            allowBuiltinFetchGit = true;
          };
          # `git` because the vault's git widgets are tested against a
          # real repository, and the sandbox has no tools it is not
          # given. `HOME` because git refuses to run without one.
          nativeBuildInputs = [pkgs.pkg-config pkgs.git];
          HOME = "/build";
          buildInputs = gpuiBuildInputs pkgs;
          # The same font directory the devShell and the packaged
          # binary use. Without it `bundled_fonts` compiles to a
          # different body, and the first run of this check found a
          # clippy warning the local gates cannot see — three
          # environments disagreeing about what is being compiled is
          # the bug, not the lint.
          CLOSURE_FONT_DIR = "${pkgs.maple-mono.NF}/share/fonts/truetype";
          buildPhase = "cargo ${args}";
          # The gate *is* the build; there is nothing to install and
          # nothing to check afterwards.
          doCheck = false;
          installPhase = "touch $out";
        };
    in {
      formatting = treefmtEval.${system}.config.build.check self;
      # The packaged binary has to carry the `git` it shells out to.
      # Without this the status fringes, the line diffs and
      # `closure sync` are all silently dead on a machine that has no
      # git of its own — silently, because a `git` that will not run
      # reads exactly like "this vault is not a repository".
      packaging = pkgs.runCommand "closure-brings-its-own-git" {} ''
        for pkg in ${self.packages.${system}.default} ${self.packages.${system}.gpui}; do
          grep -q '${pkgs.git}/bin' $pkg/bin/closure \
            || { echo "$pkg/bin/closure does not put git on PATH"; exit 1; }
        done
        touch $out
      '';
      deadnix =
        pkgs.runCommand "deadnix-check" {
          nativeBuildInputs = [pkgs.deadnix];
        } ''
          cp -r ${self}/. .
          deadnix --fail .
          touch $out
        '';
      # `statix` looks for nix antipatterns the formatter does not —
      # alejandra is already the formatter (via treefmt), so adding it
      # again as a linter would only run the same check twice.
      statix =
        pkgs.runCommand "statix-check" {
          nativeBuildInputs = [pkgs.statix];
        } ''
          cp -r ${self}/. .
          statix check .
          touch $out
        '';
      tests = cargoGate "tests" "test --workspace --locked";
      clippy = cargoGate "clippy" "clippy --workspace --all-targets --locked -- -D warnings";
    });
  };
}
