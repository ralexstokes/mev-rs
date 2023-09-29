{ pkgs, rustToolchain }:
with pkgs;
mkShell {
  buildInputs = [
    just
    mdbook
    rustToolchain
  ];
}
