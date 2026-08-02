_: {
  projectRootFile = "flake.nix";

  programs = {
    rustfmt.enable = true;
    alejandra.enable = true;
    prettier = {
      enable = true;
      includes = ["*.md" "*.yml" "*.yaml" "*.json"];
    };
    taplo.enable = true;
  };

  settings.global.excludes = [
    "LICENSE*"
    "*.lock"
    "*.org"
    ".gitignore"
    "rust-toolchain.toml"
    "fixtures/**"
    "target/**"
    "result"
    "result-*"
  ];
}
