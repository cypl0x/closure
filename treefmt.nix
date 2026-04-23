{...}: {
  projectRootFile = "flake.nix";

  programs.rustfmt.enable = true;
  programs.alejandra.enable = true;
  programs.prettier = {
    enable = true;
    includes = ["*.md" "*.yml" "*.yaml" "*.json"];
  };
  programs.taplo.enable = true;

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
