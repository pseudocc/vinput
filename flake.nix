{
  description = "vinput: voice to text input tool";

  inputs = {
    nixpkgs.url = "github:nixos/nixpkgs/nixos-unstable";
    rust-overlay = {
      url = "github:oxalica/rust-overlay";
      inputs.nixpkgs.follows = "nixpkgs";
    };
  };

  outputs =
    {
      self,
      nixpkgs,
      rust-overlay,
      ...
    }:
    let
      system = "x86_64-linux";
      pkgs = import nixpkgs {
        inherit system;
        overlays = [ rust-overlay.overlays.default ];
      };
      npuOverlay = import ./nix/overlay.nix;
      npuPkgs = import nixpkgs {
        inherit system;
        overlays = [ npuOverlay ];
      };
      runtimeModule = import ./nix/runtime-module.nix {
        inherit nixpkgs npuOverlay;
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
        openvino-npu-compiler = npuPkgs.openvino-intel-npu-compiler;
        intel-npu-driver = npuPkgs.intel-npu-driver;
        whisper-large-v3-turbo-int8-ov = pkgs.callPackage ./nix/model.nix { };
      }
      // nixpkgs.lib.optionalAttrs (builtins.hasAttr "openvino-genai" pkgs.python3Packages) {
        vinput-asr = pkgs.callPackage ./nix/asr-package.nix {
          python = pkgs.python3.withPackages (ps: [ ps.openvino-genai ]);
        };
      };

      checks.${system}.nixos-module =
        assert import ./tests/nix.nix { inherit nixpkgs system; };
        pkgs.runCommand "vinput-nixos-module-evaluation" { } ''
          touch "$out"
        '';

      overlays.openvino-npu = npuOverlay;

      nixosModules = {
        default = {
          imports = [
            ./nix/module.nix
            runtimeModule
          ];
        };
        asr = {
          imports = [
            ./nix/asr-module.nix
            runtimeModule
          ];
        };
      };
    };
}
