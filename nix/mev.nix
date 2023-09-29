{ pkgs, crane }:
with pkgs;
let
  commonArgs = {
    pname = "mev-rs";
    src = crane.cleanCargoSource (crane.path ../.);
    buildInputs = [ ] ++ lib.optionals pkgs.stdenv.isDarwin [
      darwin.apple_sdk.frameworks.Network
    ];
  };
  cargoArtifacts = crane.buildDepsOnly commonArgs;
in
crane.buildPackage (commonArgs // { inherit cargoArtifacts; })
