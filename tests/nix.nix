{
  nixpkgs,
  system ? builtins.currentSystem,
}:

let
  inherit (nixpkgs) lib;
  makeSystem =
    modules:
    lib.nixosSystem {
      inherit system;
      modules = [ { system.stateVersion = "26.05"; } ] ++ modules;
    };
  standalone =
    (makeSystem [
      ../nix/asr-module.nix
      ({ pkgs, ... }: {
        services.vinput-asr = {
          enable = true;
          device = "NPU";
          language = "en";
          port = 18081;
          python = pkgs.python3;
          npuDriver = pkgs.hello;
        };
      })
    ]).config;
  legacy =
    (makeSystem [
      ../nix/module.nix
      ({ pkgs, ... }: {
        programs.vinput = {
          enable = true;
          localAsr = {
            enable = true;
            modelPath = "/var/lib/custom-model";
            port = 18082;
            python = pkgs.python3;
          };
        };
      })
    ]).config;
  missingPython =
    (makeSystem [
      ../nix/asr-module.nix
      { services.vinput-asr.enable = true; }
    ]).config;
  disabled = (makeSystem [ ../nix/module.nix ]).config;
  pkgs = import nixpkgs { inherit system; };
in
assert standalone.services.vinput-asr.enable;
assert !(standalone ? programs.vinput);
assert
  standalone.systemd.services.vinput-asr.environment.WHISPER_MODEL_DIR
  == toString standalone.services.vinput-asr.model;
assert standalone.systemd.services.vinput-asr.environment.WHISPER_DEVICE == "NPU";
assert standalone.systemd.services.vinput-asr.environment.WHISPER_LANGUAGE == "en";
assert standalone.systemd.services.vinput-asr.environment.WHISPER_PORT == "18081";
assert standalone.systemd.services.vinput-asr.environment.WHISPER_HOST == "127.0.0.1";
assert
  standalone.systemd.services.vinput-asr.environment.ZE_ENABLE_ALT_DRIVERS
  == "${lib.getLib pkgs.hello}/lib/libze_intel_npu.so";
assert standalone.systemd.services.vinput-asr.serviceConfig.DynamicUser;
assert standalone.systemd.services.vinput-asr.serviceConfig.IPAddressDeny == "any";
assert legacy.services.vinput-asr.enable;
assert legacy.systemd.services.vinput-asr.environment.WHISPER_MODEL_DIR == "/var/lib/custom-model";
assert
  legacy.systemd.user.services.vinput.environment.VINPUT_ASR_URL
  == "http://127.0.0.1:18082/v1/audio/transcriptions";
assert legacy.programs.ydotool.enable;
assert
  (builtins.any (
    entry: !entry.assertion && lib.hasPrefix "services.vinput-asr requires" entry.message
  ) missingPython.assertions) == !(builtins.hasAttr "openvino-genai" pkgs.python3Packages);
assert !disabled.services.vinput-asr.enable;
assert !(disabled.systemd.services ? vinput-asr);
true
