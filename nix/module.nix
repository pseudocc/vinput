{
  config,
  lib,
  pkgs,
  ...
}:

let
  cfg = config.programs.vinput;
in
{
  imports = [
    ./asr-module.nix
  ]
  ++
    map
      (
        name:
        lib.mkAliasOptionModule [ "programs" "vinput" "localAsr" name ] [ "services" "vinput-asr" name ]
      )
      [
        "enable"
        "modelPath"
        "device"
        "language"
        "port"
        "python"
      ];

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
      default = "http://127.0.0.1:${toString config.services.vinput-asr.port}/v1/audio/transcriptions";
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
  };

  config = lib.mkIf cfg.enable {
    programs.ydotool.enable = true;
    environment.systemPackages = [ cfg.package ];

    systemd.user.services.vinput = {
      description = "vinput voice input listener";
      wantedBy = [ "default.target" ];
      environment = {
        VINPUT_ASR_URL = cfg.asrUrl;
        YDOTOOL_SOCKET = config.environment.variables.YDOTOOL_SOCKET;
      }
      // lib.optionalAttrs (cfg.model != null) {
        VINPUT_MODEL = cfg.model;
      }
      // lib.optionalAttrs (cfg.language != null) {
        VINPUT_LANGUAGE = cfg.language;
      }
      // lib.optionalAttrs (cfg.captureDevice != null) {
        VINPUT_CAPTURE_DEVICE = cfg.captureDevice;
      };
      serviceConfig = {
        ExecStart = "${lib.getExe cfg.package} listen";
        Restart = "on-failure";
        RestartSec = 2;
      };
    };
  };
}
