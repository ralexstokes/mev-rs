{ pkgs, crane }:
with pkgs;
let
  commonArgs = {
    pname = "mev-rs";
    src = crane.cleanCargoSource (crane.path ../.);
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
