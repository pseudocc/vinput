{ nixpkgs, npuOverlay }:

{
  config,
  lib,
  pkgs,
  ...
}:

let
  cfg = config.services.vinput-asr;
  device = lib.toUpper cfg.device;
  host = pkgs.stdenv.hostPlatform;
  build = pkgs.stdenv.buildPlatform;
  # Elaborated platforms contain derived fields and functions from the host
  # nixpkgs. Re-elaborate only the target triple and GCC settings in the
  # runtime nixpkgs, so its native/cross comparisons use its own defaults.
  runtimeSystem = platform: { inherit (platform) config gcc; };
  usesNpu =
    host.isLinux
    && host.isx86_64
    && (
      lib.hasInfix "NPU" device || lib.hasPrefix "AUTO" device || device == "MULTI" || device == "HETERO"
    );
  runtimePkgs = import nixpkgs {
    localSystem = runtimeSystem build;
    crossSystem = runtimeSystem host;
    overlays = lib.optional usesNpu npuOverlay;
  };
in
{
  key = toString ./runtime-module.nix;
  config = lib.mkIf cfg.enable {
    services.vinput-asr = {
      python = lib.mkDefault (runtimePkgs.python3.withPackages (ps: [ ps.openvino-genai ]));
      npuDriver = lib.mkIf usesNpu (lib.mkDefault runtimePkgs.intel-npu-driver);
    };
  };
}
