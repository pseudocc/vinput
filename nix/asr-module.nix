{
  config,
  lib,
  pkgs,
  ...
}:

let
  cfg = config.services.vinput-asr;
  hasGenai = builtins.hasAttr "openvino-genai" pkgs.python3Packages;
  asrPython =
    if cfg.python != null then
      cfg.python
    else if hasGenai then
      pkgs.python3.withPackages (ps: [ ps.openvino-genai ])
    else
      pkgs.python3;
  asrPackage = pkgs.callPackage ./asr-package.nix { python = asrPython; };
in
{
  options.services.vinput-asr = {
    enable = lib.mkEnableOption "the local OpenVINO ASR service";

    model = lib.mkOption {
      type = lib.types.package;
      default = pkgs.callPackage ./model.nix { };
      defaultText = lib.literalExpression "pkgs.callPackage ./model.nix { }";
      description = ''
        Package containing the OpenVINO Whisper model at its output root.
        Ignored when modelPath is set.
      '';
    };

    modelPath = lib.mkOption {
      type = lib.types.nullOr (lib.types.strMatching "/.*");
      default = null;
      example = "/var/lib/whisper/model";
      description = ''
        Absolute path to an existing OpenVINO Whisper model directory, overriding
        model. The directory must be readable by the dynamically allocated service
        user. Home directories are mounted read-only for the service.
      '';
    };

    device = lib.mkOption {
      type = lib.types.str;
      default = "CPU";
      example = "NPU";
      description = "OpenVINO device. Non-CPU devices require a compatible runtime and host drivers.";
    };

    language = lib.mkOption {
      type = lib.types.nullOr (lib.types.strMatching "[a-zA-Z]{2,3}");
      default = null;
      description = "Optional default language for the ASR server.";
    };

    port = lib.mkOption {
      type = lib.types.ints.between 1 65535;
      default = 18080;
      description = "Loopback TCP port for the ASR server.";
    };

    python = lib.mkOption {
      type = lib.types.nullOr lib.types.package;
      default = null;
      description = ''
        Python environment containing openvino_genai, with an executable at
        bin/python3. If null, use python3.withPackages with openvino-genai from
        nixpkgs. Set this when those bindings are unavailable in your nixpkgs.
      '';
    };

    npuDriver = lib.mkOption {
      type = lib.types.nullOr lib.types.package;
      default = null;
      description = ''
        Optional Intel NPU driver package used to set ZE_ENABLE_ALT_DRIVERS.
        Its library output must contain lib/libze_intel_npu.so. Configure a
        compatible OpenVINO Python environment and device permissions separately.
      '';
    };
  };

  config = lib.mkIf cfg.enable {
    assertions = [
      {
        assertion = cfg.python != null || hasGenai;
        message = "services.vinput-asr requires python3Packages.openvino-genai or a services.vinput-asr.python environment containing openvino_genai.";
      }
    ];

    systemd.services.vinput-asr = {
      description = "vinput local OpenVINO ASR server";
      wantedBy = [ "multi-user.target" ];
      environment = {
        WHISPER_MODEL_DIR = if cfg.modelPath != null then cfg.modelPath else toString cfg.model;
        WHISPER_DEVICE = cfg.device;
        WHISPER_HOST = "127.0.0.1";
        WHISPER_PORT = toString cfg.port;
        OV_CACHE_DIR = "/var/lib/vinput-asr";
      }
      // lib.optionalAttrs (cfg.language != null) {
        WHISPER_LANGUAGE = cfg.language;
      }
      // lib.optionalAttrs (cfg.npuDriver != null) {
        ZE_ENABLE_ALT_DRIVERS = "${lib.getLib cfg.npuDriver}/lib/libze_intel_npu.so";
      };
      serviceConfig = {
        ExecStart = lib.getExe asrPackage;
        DynamicUser = true;
        StateDirectory = "vinput-asr";
        NoNewPrivileges = true;
        ProtectSystem = "strict";
        ProtectHome = "read-only";
        PrivateTmp = true;
        IPAddressDeny = "any";
        IPAddressAllow = "localhost";
        Restart = "on-failure";
        RestartSec = 5;
      };
    };
  };
}
