{
  description = "vinput: voice to text input tool";

  inputs = {
    system.url = "path:/etc/nixos";
    nixpkgs.follows = "system/nixpkgs";
    rust-overlay = {
      url = "github:oxalica/rust-overlay";
      inputs.nixpkgs.follows = "nixpkgs";
    };
  };

  outputs = { self, nixpkgs, rust-overlay, ... }:
    let
      system = "x86_64-linux";
      pkgs = import nixpkgs {
        inherit system;
        overlays = [ rust-overlay.overlays.default ];
      };
      rust-toolchain = pkgs.rust-bin.stable.latest.default;
    in
    {
      devShells.${system} = {
        default = pkgs.mkShell {
          name = "rust-stable";
          packages = with pkgs; [
            crate2nix
            rust-toolchain
            nodejs
            wasm-bindgen-cli
          ];
        };
      };

      packages.${system} = {
        default = pkgs.callPackage ./nix/package.nix { };
      };

      nixosModules.default = import ./nix/module.nix;
    };
}
