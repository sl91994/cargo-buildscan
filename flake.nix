{
  description = "rust environment";

  inputs = {
    nixpkgs.url = "github:nixos/nixpkgs?ref=nixos-unstable";
    rust-overlay = {
      url = "github:oxalica/rust-overlay";
      inputs.nixpkgs.follows = "nixpkgs";
    };
  };

  outputs = { self, nixpkgs, rust-overlay }: let
    system = "x86_64-linux";
    
    pkgs = import nixpkgs {
      inherit system;
      overlays = [ (import rust-overlay) ];
    };
    
    name = "rust";

    rustToolchain = pkgs.rust-bin.fromRustupToolchainFile ./rust-toolchain.toml;

  in {
    devShells.${system}.default = pkgs.mkShell {
      inherit name;
      
      buildInputs = [
        rustToolchain
      ];

      nativeBuildInputs = [
        pkgs.pkg-config
      ];

      env.RUST_SRC_PATH = "${rustToolchain}/lib/rustlib/src/rust/library";

      shellHook = ''
        echo "Rust: $(cargo --version)"
      '';
    };
  };
}
