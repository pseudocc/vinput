{ config, lib, pkgs, ... }:

let
  cfg = config.programs.vinput;
  asr = cfg.localAsr;
  hasGenai = builtins.hasAttr "openvino-genai" pkgs.python3Packages;
  defaultAsrPython = pkgs.python3.withPackages (ps: [ ps.numpy ps.openvino-genai ]);
  asrPython = if asr.python != null then asr.python else if hasGenai then defaultAsrPython else pkgs.python3;
in
{
  options.programs.vinput = {
    enable = lib.mkEnableOption ''
      the vinput voice input listener. Each user running the listener must be
      a member of the group configured by {option}`programs.ydotool.group`
    '';

    package = lib.mkOption {
      type = lib.types.package;
      default = pkgs.callPackage ./package.nix { };
      defaultText = lib.literalExpression "pkgs.callPackage ./package.nix { }";
      description = "The vinput package to install and run.";
    };

    asrUrl = lib.mkOption {
      type = lib.types.str;
      default = "http://127.0.0.1:${toString asr.port}/v1/audio/transcriptions";
      defaultText = lib.literalExpression ''"http://127.0.0.1:18080/v1/audio/transcriptions"'';
      description = "OpenAI-compatible audio transcription endpoint. By default, uses the local ASR port.";
    };

    model = lib.mkOption {
      type = lib.types.nullOr lib.types.str;
      default = null;
      description = "ASR model name; null uses vinput's default.";
    };

    language = lib.mkOption {
      type = lib.types.nullOr lib.types.str;
      default = null;
      description = "Optional transcription language.";
    };

    captureDevice = lib.mkOption {
      type = lib.types.nullOr lib.types.str;
      default = null;
      description = "Optional ALSA capture device passed to vinput.";
    };

    localAsr = {
      enable = lib.mkEnableOption "the local OpenVINO ASR service";

      modelPath = lib.mkOption {
        type = lib.types.nullOr (lib.types.strMatching "/.*");
        default = null;
        example = "/var/lib/whisper/model";
        description = "Absolute path to an existing OpenVINO Whisper model directory, readable by the ASR service.";
      };

      device = lib.mkOption {
        type = lib.types.str;
        default = "CPU";
        example = "NPU";
        description = "OpenVINO device. Non-CPU devices require support from the selected nixpkgs and host drivers.";
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
          Python environment containing numpy and openvino_genai. If null, use
          python3.withPackages with numpy and openvino-genai from nixpkgs.
          Set this when those bindings are not available in your nixpkgs.
        '';
      };
    };
  };

  config = lib.mkIf cfg.enable {
    assertions = [
      {
        assertion = !asr.enable || asr.modelPath != null;
        message = "programs.vinput.localAsr.modelPath must be set when localAsr is enabled.";
      }
      {
        assertion = !asr.enable || asr.python != null || hasGenai;
        message = "programs.vinput.localAsr requires python3Packages.openvino-genai or a programs.vinput.localAsr.python environment containing openvino_genai and numpy.";
      }
    ];

    programs.ydotool.enable = true;
    environment.systemPackages = [ cfg.package ];

    systemd.user.services.vinput = {
      description = "vinput voice input listener";
      wantedBy = [ "default.target" ];
      environment = {
        VINPUT_ASR_URL = cfg.asrUrl;
        YDOTOOL_SOCKET = config.environment.variables.YDOTOOL_SOCKET;
      } // lib.optionalAttrs (cfg.model != null) {
        VINPUT_MODEL = cfg.model;
      } // lib.optionalAttrs (cfg.language != null) {
        VINPUT_LANGUAGE = cfg.language;
      } // lib.optionalAttrs (cfg.captureDevice != null) {
        VINPUT_CAPTURE_DEVICE = cfg.captureDevice;
      };
      serviceConfig = {
        ExecStart = "${lib.getExe cfg.package} listen";
        Restart = "on-failure";
        RestartSec = 2;
      };
    };

    systemd.services.vinput-asr = lib.mkIf asr.enable {
      description = "vinput local OpenVINO ASR server";
      wantedBy = [ "multi-user.target" ];
      environment = {
        WHISPER_MODEL_DIR = if asr.modelPath == null then "" else asr.modelPath;
        WHISPER_DEVICE = asr.device;
        WHISPER_HOST = "127.0.0.1";
        WHISPER_PORT = toString asr.port;
        OV_CACHE_DIR = "/var/lib/vinput-asr";
      } // lib.optionalAttrs (asr.language != null) {
        WHISPER_LANGUAGE = asr.language;
      };
      serviceConfig = {
        ExecStart = "${lib.getExe asrPython} ${../asr/server.py}";
        DynamicUser = true;
        StateDirectory = "vinput-asr";
        Restart = "on-failure";
        RestartSec = 5;
      };
    };
  };
}
