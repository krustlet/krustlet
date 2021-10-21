{
  inputs = {
    nixpkgs.url = "nixpkgs/nixpkgs-unstable";
    rust-overlay.url = "github:oxalica/rust-overlay";
    flake-utils.url = "github:numtide/flake-utils";
  };

  outputs = { self, rust-overlay, nixpkgs, flake-utils }:
    flake-utils.lib.eachDefaultSystem (system:
      let
        overlays = [ (import rust-overlay) ];
        pkgs = import nixpkgs {
          inherit system overlays;
        };
        devRustNightly = pkgs.rust-bin.nightly."2021-08-31".default.override {
          extensions = [ "rust-src" ];
          targets = [ "wasm32-unknown-unknown" ];
        };
        setupScript = pkgs.writeShellScriptBin "setup" ''
          kind create cluster
          kubectl cluster-info --context kind-kind
        '';
        joinClusterScript = pkgs.writeShellScriptBin "join-cluster" ''
          kind create cluster
          kubectl cluster-info --context kind-kind
          kubectl get nodes -o wide
          just run
        '';
        approveCertificateScript = pkgs.writeShellScriptBin "approve-csr" ''
          kubectl get csr
          kubectl certificate approve krustlet-wasi-tls
          kubectl get csr
        '';
        cleanUpScript = pkgs.writeShellScriptBin "clean-up" ''
          kind delete cluster
          rm -rf ~/.krustlet
        '';
      in
      with pkgs;
      {
        devShell = mkShell {
          buildInputs = [
            openssl
            pkg-config
            devRustNightly
            just

            setupScript
            joinClusterScript
            approveCertificateScript
            cleanUpScript
          ];

          # Should be in the same class B subnet with the control plane
          KRUSTLET_NODE_IP = "172.18.0.1";
          KRUSTLET_BOOTSTRAP_FILE = "~/.krustlet/config/bootstrap.conf";
          KRUSTLET_HOSTNAME = "krustlet-wasi";
          KRUSTLET_NODE_NAME = "krustlet-wasi";
        };
      }
    );
}
