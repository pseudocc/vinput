{
  nixpkgs,
  # Override to exercise integration with a different host nixpkgs revision.
  hostNixpkgs ? nixpkgs,
  system ? builtins.currentSystem,
}:

let
  inherit (nixpkgs) lib;
  npuOverlay = import ../nix/overlay.nix;
  runtimeModule = import ../nix/runtime-module.nix { inherit nixpkgs npuOverlay; };
  # Make reuse of derived fields from the host nixpkgs fail even when the
  # test suite is run with just one nixpkgs revision.
  poisonedRuntimeModule =
    {
      config,
      lib,
      pkgs,
      ...
    }@args:
    runtimeModule (
      args
      // {
        pkgs = pkgs // {
          stdenv = pkgs.stdenv // {
            buildPlatform = poisonPlatform pkgs.stdenv.buildPlatform;
            hostPlatform = poisonPlatform pkgs.stdenv.hostPlatform;
          };
        };
      }
    );
  poisonPlatform =
    platform:
    platform
    // {
      isGnu = throw "reused a foreign derived platform field";
      canExecute = _: throw "reused a foreign platform function";
    };
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
  automaticNpu =
    (makeSystem [
      ../nix/asr-module.nix
      runtimeModule
      {
        services.vinput-asr = {
          enable = true;
          device = "NPU";
          language = "en";
        };
      }
    ]).config;
  automaticCpu =
    (makeSystem [
      ../nix/asr-module.nix
      runtimeModule
      { services.vinput-asr.enable = true; }
    ]).config;
  automaticDevice =
    (makeSystem [
      ../nix/asr-module.nix
      runtimeModule
      {
        services.vinput-asr = {
          enable = true;
          device = "AUTO";
        };
      }
    ]).config;
  nonIntel =
    (lib.nixosSystem {
      system = "aarch64-linux";
      modules = [
        ../nix/asr-module.nix
        runtimeModule
        {
          system.stateVersion = "26.05";
          services.vinput-asr = {
            enable = true;
            device = "AUTO";
          };
        }
      ];
    }).config;
  cross =
    (lib.nixosSystem {
      system = "x86_64-linux";
      modules = [
        ../nix/asr-module.nix
        runtimeModule
        {
          system.stateVersion = "26.05";
          nixpkgs.crossSystem.config = "aarch64-unknown-linux-gnu";
          services.vinput-asr = {
            enable = true;
            device = "AUTO";
          };
        }
      ];
    }).config;
  foreignSystem =
    modules:
    (hostNixpkgs.lib.nixosSystem {
      modules = [
        ../nix/asr-module.nix
        { system.stateVersion = "26.05"; }
      ]
      ++ modules;
    }).config;
  foreignNative = foreignSystem [
    runtimeModule
    {
      nixpkgs.hostPlatform = "x86_64-linux";
      services.vinput-asr = {
        enable = true;
        device = "NPU";
      };
    }
  ];
  poisonedNative = foreignSystem [
    poisonedRuntimeModule
    {
      nixpkgs.hostPlatform = "x86_64-linux";
      services.vinput-asr = {
        enable = true;
        device = "NPU";
      };
    }
  ];
  foreignCross = foreignSystem [
    poisonedRuntimeModule
    {
      nixpkgs.buildPlatform = "x86_64-linux";
      nixpkgs.hostPlatform = "aarch64-linux";
      services.vinput-asr = {
        enable = true;
        device = "AUTO";
      };
    }
  ];
  crossNpu = foreignSystem [
    poisonedRuntimeModule
    {
      nixpkgs.buildPlatform = "aarch64-linux";
      nixpkgs.hostPlatform = "x86_64-linux";
      services.vinput-asr = {
        enable = true;
        device = "NPU";
      };
    }
  ];
  tunedGcc = {
    arch = "x86-64-v3";
    tune = "skylake";
  };
  tunedNative = foreignSystem [
    poisonedRuntimeModule
    {
      nixpkgs.hostPlatform = {
        system = "x86_64-linux";
        gcc = tunedGcc;
      };
      services.vinput-asr = {
        enable = true;
        device = "NPU";
      };
    }
  ];
  tunedCross = foreignSystem [
    poisonedRuntimeModule
    {
      nixpkgs.buildPlatform = "x86_64-linux";
      nixpkgs.hostPlatform = {
        system = "x86_64-linux";
        gcc = tunedGcc;
      };
      services.vinput-asr = {
        enable = true;
        device = "CPU";
      };
    }
  ];
  tunedArmGcc = {
    arch = "armv8-a";
    tune = "cortex-a72";
  };
  tunedForeignCross = foreignSystem [
    poisonedRuntimeModule
    {
      nixpkgs.buildPlatform = {
        system = "x86_64-linux";
        gcc = tunedGcc;
      };
      nixpkgs.hostPlatform = {
        system = "aarch64-linux";
        gcc = tunedArmGcc;
      };
      services.vinput-asr = {
        enable = true;
        device = "AUTO";
      };
    }
  ];
  overrideNpu =
    (makeSystem [
      ../nix/asr-module.nix
      runtimeModule
      ({ pkgs, ... }: {
        services.vinput-asr = {
          enable = true;
          device = "NPU";
          python = pkgs.python3;
          npuDriver = null;
        };
      })
    ]).config;
  bothModules =
    (makeSystem [
      ../nix/module.nix
      ../nix/asr-module.nix
      runtimeModule
      runtimeModule
      {
        programs.vinput.enable = true;
        services.vinput-asr = {
          enable = true;
          device = "NPU";
          language = "en";
        };
      }
    ]).config;
  disabled = (makeSystem [ ../nix/module.nix ]).config;
  pkgs = import nixpkgs { inherit system; };
  npu = import nixpkgs {
    inherit system;
    overlays = [ npuOverlay ];
  };
  crossPkgs = import nixpkgs {
    localSystem.system = "x86_64-linux";
    crossSystem.config = "aarch64-unknown-linux-gnu";
  };
  crossNpuPkgs = import nixpkgs {
    localSystem.system = "aarch64-linux";
    crossSystem.config = "x86_64-unknown-linux-gnu";
    overlays = [ npuOverlay ];
  };
  tunedNativePkgs = import nixpkgs {
    localSystem = {
      config = "x86_64-unknown-linux-gnu";
      gcc = tunedGcc;
    };
    overlays = [ npuOverlay ];
  };
  tunedCrossPkgs = import nixpkgs {
    localSystem.config = "x86_64-unknown-linux-gnu";
    crossSystem = {
      config = "x86_64-unknown-linux-gnu";
      gcc = tunedGcc;
    };
  };
  tunedForeignCrossPkgs = import nixpkgs {
    localSystem = {
      config = "x86_64-unknown-linux-gnu";
      gcc = tunedGcc;
    };
    crossSystem = {
      config = "aarch64-unknown-linux-gnu";
      gcc = tunedArmGcc;
    };
  };
  incompatible = import nixpkgs {
    inherit system;
    overlays = [
      (final: prev: {
        openvino = prev.openvino.overrideAttrs (_: {
          version = "0.0";
        });
      })
      npuOverlay
    ];
  };
  upstreamCompiler = import nixpkgs {
    inherit system;
    overlays = [
      (final: prev: { openvino-intel-npu-compiler = prev.hello; })
      npuOverlay
    ];
  };
in
assert !(builtins.tryEval incompatible.openvino.drvPath).success;
assert !(builtins.tryEval upstreamCompiler.openvino-intel-npu-compiler.drvPath).success;
assert npu.openvino.drvPath != pkgs.openvino.drvPath;
assert npu.intel-npu-driver.drvPath != pkgs.intel-npu-driver.drvPath;
assert builtins.elem npu.openvino.drvPath (
  map (input: input.drvPath) npu.openvino-genai.buildInputs
);
assert builtins.elem npu.openvino.drvPath (
  map (input: input.drvPath) npu.openvino-tokenizers.buildInputs
);
assert toString npu.python3Packages.openvino-genai.src == toString npu.openvino-genai.python;
assert toString npu.python3Packages.openvino.src == toString npu.openvino.python;
assert builtins.elem npu.python3Packages.openvino-tokenizers.drvPath (
  map (input: input.drvPath) npu.python3Packages.openvino-genai.propagatedBuildInputs
);
assert builtins.elem npu.python3Packages.openvino.drvPath (
  map (input: input.drvPath) npu.python3Packages.openvino-tokenizers.propagatedBuildInputs
);
assert lib.hasInfix
  (builtins.unsafeDiscardStringContext "${lib.getLib npu.openvino-intel-npu-compiler}/lib")
  npu.intel-npu-driver.postPatch;
assert automaticNpu.services.vinput-asr.npuDriver.drvPath == npu.intel-npu-driver.drvPath;
assert
  automaticNpu.services.vinput-asr.python.drvPath
  == (npu.python3.withPackages (ps: [ ps.openvino-genai ])).drvPath;
assert automaticCpu.services.vinput-asr.npuDriver == null;
assert
  automaticCpu.services.vinput-asr.python.drvPath
  == (pkgs.python3.withPackages (ps: [ ps.openvino-genai ])).drvPath;
assert automaticDevice.services.vinput-asr.npuDriver.drvPath == npu.intel-npu-driver.drvPath;
assert nonIntel.services.vinput-asr.npuDriver == null;
assert nonIntel.services.vinput-asr.python.system == "aarch64-linux";
assert cross.services.vinput-asr.npuDriver == null;
assert cross.services.vinput-asr.python.system == "x86_64-linux";
assert
  cross.services.vinput-asr.python.drvPath
  == (crossPkgs.python3.withPackages (ps: [ ps.openvino-genai ])).drvPath;
assert foreignNative.services.vinput-asr.npuDriver.drvPath == npu.intel-npu-driver.drvPath;
assert
  foreignNative.services.vinput-asr.python.drvPath == automaticNpu.services.vinput-asr.python.drvPath;
assert poisonedNative.services.vinput-asr.npuDriver.drvPath == npu.intel-npu-driver.drvPath;
assert
  poisonedNative.services.vinput-asr.python.drvPath
  == automaticNpu.services.vinput-asr.python.drvPath;
assert foreignCross.services.vinput-asr.npuDriver == null;
assert foreignCross.services.vinput-asr.python.drvPath == cross.services.vinput-asr.python.drvPath;
assert crossNpu.services.vinput-asr.npuDriver.drvPath == crossNpuPkgs.intel-npu-driver.drvPath;
assert
  crossNpu.services.vinput-asr.python.drvPath
  == (crossNpuPkgs.python3.withPackages (ps: [ ps.openvino-genai ])).drvPath;
assert
  tunedNative.services.vinput-asr.npuDriver.drvPath == tunedNativePkgs.intel-npu-driver.drvPath;
assert
  tunedNative.services.vinput-asr.python.drvPath
  == (tunedNativePkgs.python3.withPackages (ps: [ ps.openvino-genai ])).drvPath;
assert
  tunedCross.services.vinput-asr.python.drvPath
  == (tunedCrossPkgs.python3.withPackages (ps: [ ps.openvino-genai ])).drvPath;
assert tunedForeignCross.services.vinput-asr.npuDriver == null;
assert
  tunedForeignCross.services.vinput-asr.python.drvPath
  == (tunedForeignCrossPkgs.python3.withPackages (ps: [ ps.openvino-genai ])).drvPath;
assert overrideNpu.services.vinput-asr.python.drvPath == pkgs.python3.drvPath;
assert overrideNpu.services.vinput-asr.npuDriver == null;
assert bothModules.services.vinput-asr.npuDriver.drvPath == npu.intel-npu-driver.drvPath;
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
