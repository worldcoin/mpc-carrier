{
  description = "Worldcoin MPC communication channel";

  inputs = {
    utils.url = "github:numtide/flake-utils";
    nixpkgs.url = "nixpkgs/nixos-23.11";
    fenix = {
      url = "github:nix-community/fenix";
      inputs.nixpkgs.follows = "nixpkgs";
    };
  };

  outputs = { self, utils, nixpkgs, fenix }:
    utils.lib.eachDefaultSystem (system:
      let
        rustChannel = {
          channel = "1.75";
          sha256 = "SXRtAuO4IqNOQq+nLbrsDFbVk+3aVA8NNpSZsKlVH/8=";
        };
        pkgs = nixpkgs.legacyPackages.${system};
        rustToolchain = with fenix.packages.${system}; combine
          (with toolchainOf rustChannel; [
            cargo
            clippy
            rustc
            rustfmt
            rust-src
          ]);
        rustAnalyzer = fenix.packages.${system}.rust-analyzer;
      in
      {
        devShells = {
          default = pkgs.mkShell ({
            nativeBuildInputs = [
              rustToolchain
              rustAnalyzer
              pkgs.protobuf
            ];
            RUST_SRC_PATH = "${rustToolchain}/lib/rustlib/src/rust/library";
          });
        };
      }
    );
}
