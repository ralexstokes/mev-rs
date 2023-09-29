{ pkgs, crane }:
with pkgs;
let
  commonArgs = {
    pname = "mev-rs";
    src = crane.cleanCargoSource (crane.path ../.);
    buildInputs = lib.optionals pkgs.stdenv.isLinux [
      openssl
    ] ++ lib.optionals pkgs.stdenv.isDarwin [
      darwin.apple_sdk.frameworks.Network
    ];
    nativeBuildInputs = lib.optionals pkgs.stdenv.isLinux [
      perl
      clang
    ];
    LIBCLANG_PATH = "${llvmPackages.libclang.lib}/lib";
  };
  cargoArtifacts = crane.buildDepsOnly commonArgs;
in
crane.buildPackage (commonArgs // { inherit cargoArtifacts; })
