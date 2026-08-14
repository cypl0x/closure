# The Android SDK `just apk` builds against.
#
# Pinned here rather than installed by gradle, because the store is
# read-only: when a component is missing, AGP does not fall back, it
# fails with "The SDK directory is not writable". Every version below
# was added because a build asked for it by name, so the list is a
# record of what Flutter 3.44 actually wants, not a guess.
#
# Unfree and licence-accepting on purpose. This is outside the hermetic
# gate exactly as the rest of the Flutter shell is (I10) — `nix flake
# check` never evaluates this file.
let
  flake = builtins.getFlake "nixpkgs";
  pkgs = import flake.outPath {
    system = "x86_64-linux";
    config.allowUnfree = true;
    config.android_sdk.accept_license = true;
  };
in
  (pkgs.androidenv.composeAndroidPackages {
    platformVersions = ["34" "35" "36"];
    buildToolsVersions = ["34.0.0" "35.0.0" "36.0.0"];
    includeNDK = true;
    # The version Flutter's gradle plugin names. It applies this to
    # every subproject, so pinning it in build.gradle.kts is not enough.
    ndkVersions = ["28.2.13676358"];
    cmakeVersions = ["3.22.1"];
    cmdLineToolsVersion = "13.0";
  })
  .androidsdk
