{
  description = "flake for `mev-rs` repo";

  inputs = {
    nixpkgs.url = "github:NixOS/nixpkgs/nixpkgs-unstable";
    flake-utils.url = "github:numtide/flake-utils";
    rust-overlay = {
      url = "github:oxalica/rust-overlay";
      inputs = {
        nixpkgs.follows = "nixpkgs";
        flake-utils.follows = "flake-utils";
      };
    };
    crane = {
      url = "github:ipetkov/crane";
      inputs.nixpkgs.follows = "nixpkgs";
    };
  };

  outputs = { self, flake-utils, nixpkgs, rust-overlay, crane }:
    {
      overlays.default = final: prev: {
        inherit (self.packages.${final.system}) mev-rs;
      };
      nixosModules.default = import ./nix/module.nix;
    } //
    flake-utils.lib.eachDefaultSystem
      (system:
        let
          overlays = [ (import rust-overlay) ];
          pkgs = import nixpkgs { inherit system overlays; };
          rustToolchain = pkgs.pkgsBuildHost.rust-bin.fromRustupToolchainFile ./rust-toolchain.toml;
          craneLib = (crane.mkLib pkgs).overrideToolchain rustToolchain;
          mev-rs = pkgs.callPackage ./nix/mev-rs.nix { inherit pkgs; crane = craneLib; };
        in
        {
          packages = { inherit mev-rs; };
          devShells.default = import ./shell.nix { inherit pkgs; };
        });
}
