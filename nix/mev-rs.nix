{ pkgs, crane, features ? "" }:
with pkgs;
let
  feature-set = if features != "" then "--no-default-features --features ${features}" else "";
  commonArgs = {
    pname = "mev-rs";
    src = crane.cleanCargoSource (crane.path ../.);
    CARGO_PROFILE = "maxperf";
    RUSTFLAGS = "-C target-cpu=native";
    cargoExtraArgs = "--locked ${feature-set}";
    buildInputs = lib.optionals pkgs.stdenv.isLinux [
      openssl
    ] ++ lib.optionals pkgs.stdenv.isDarwin [
      darwin.apple_sdk.frameworks.CFNetwork
      darwin.apple_sdk.frameworks.SystemConfiguration
    ];
    nativeBuildInputs = lib.optionals pkgs.stdenv.isLinux [
      clang
      perl
      pkg-config
    ];
    LIBCLANG_PATH = "${llvmPackages.libclang.lib}/lib";
  };
  cargoArtifacts = crane.buildDepsOnly commonArgs;
in
crane.buildPackage (commonArgs // { inherit cargoArtifacts; })
