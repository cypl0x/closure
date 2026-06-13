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
    }: {
      default = pkgs.mkShell {
        packages = [
          rustToolchain.${system}
          pkgs.cargo-nextest
          pkgs.cargo-fuzz
          pkgs.just
          pkgs.ripgrep
          pkgs.fd
          pkgs.treefmt
          pkgs.alejandra
          pkgs.deadnix
          pkgs.taplo
          pkgs.prettier
        ];

        env = {
          RUST_BACKTRACE = "1";
        };
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
      formatting = treefmtEval.${system}.config.build.check self;
      deadnix =
        pkgs.runCommand "deadnix-check" {
          nativeBuildInputs = [pkgs.deadnix];
        } ''
          cp -r ${self}/. .
          deadnix --fail .
          touch $out
        '';
      # Wasm/WASI core (ROADMAP): org + core compile to wasm32-wasip1 (feature-gate fs/process in crates that have it; org/core are pure).
      # CI target check in flake.
      # The rust-toolchain.toml includes the target; fenix pulls it.
      wasm =
        pkgs.runCommand "wasm-check" {
          nativeBuildInputs = [ rustToolchain.${system} ];
        } ''
          cargo check --target wasm32-wasip1 -p closure-org -p closure-core
          touch $out
        '';
    });
  };
}
