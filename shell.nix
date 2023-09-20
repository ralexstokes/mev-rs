{ pkgs, lib }:
with pkgs;
mkShell {
  buildInputs = lib.lists.optionals stdenv.isDarwin [
    darwin.apple_sdk.frameworks.Network
  ] ++ [
    iconv
    mdbook
    zlib
  ];
}
