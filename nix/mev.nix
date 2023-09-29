{ pkgs, crane }:
with pkgs;
let
  commonArgs = {
    pname = "mev-rs";
    src = crane.cleanCargoSource (crane.path ../.);
    buildInputs = [ ] ++ lib.optionals pkgs.stdenv.isDarwin [
      libiconv
    ];
    nativeBuildInputs = [
      # pkgs.rustPlatform.bindgenHook
    ];
  };
  cargoArtifacts = crane.buildDepsOnly commonArgs;
in
crane.buildPackage (commonArgs // { inherit cargoArtifacts; })
