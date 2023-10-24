{
  description = "flake for `mev-rs` repo";

  inputs = {
    nixpkgs.url = "github:NixOS/nixpkgs/nixpkgs-unstable";
    rust-overlay = {
      url = "github:oxalica/rust-overlay";
      inputs.nixpkgs.follows = "nixpkgs";
    };
    crane = {
      url = "github:ipetkov/crane";
      inputs.nixpkgs.follows = "nixpkgs";
    };
  };

  outputs = { self, flake-utils, nixpkgs, rust-overlay, crane }:
    let
      overlays = [ (import rust-overlay) ];
      pkgs-for-system = system: import nixpkgs { inherit system overlays; };
      mev-rs = system:
        let
          pkgs = pkgs-for-system system;
          rustToolchain = pkgs.pkgsBuildHost.rust-bin.fromRustupToolchainFile ./rust-toolchain.toml;
          craneLib = (crane.mkLib pkgs).overrideToolchain rustToolchain;
        in
        pkgs.callPackage ./nix/mev-rs.nix { inherit pkgs; crane = craneLib; };
    in
    {
      nixosModules.mev-rs = import ./nix/module.nix { inherit mev-rs; };
      nixosModules.default = self.nixosModules.mev-rs;

      packages.x86_64-darwin.mev-rs = mev-rs "x86_64-darwin";
      packages.x86_64-darwin.default = self.packages.x86_64-darwin.mev-rs;

      packages.aarch64-darwin.mev-rs = mev-rs "aarch64-darwin";
      packages.aarch64-darwin.default = self.packages.aarch64-darwin.mev-rs;

      packages.x86_64-linux.mev-rs = mev-rs "x86_64-linux";
      packages.x86_64-linux.default = self.packages.x86_64-linux.mev-rs;

      devShells.x86_64-darwin.default = import ./shell.nix { pkgs = pkgs-for-system "x86_64-darwin"; };
      devShells.aarch64-darwin.default = import ./shell.nix { pkgs = pkgs-for-system "aarch64-darwin"; };
      devShells.x86_64-linux.default = import ./shell.nix { pkgs = pkgs-for-system "x86_64-linux"; };
    };
}
