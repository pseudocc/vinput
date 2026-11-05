# vinput

Push-to-talk voice input for Linux. A Rust user daemon records the microphone,
sends a WAV to a transcription service, and types the response with `ydotool`.
The optional local service keeps an OpenVINO GenAI Whisper model loaded.

## NixOS

Add this flake as an input and import its module. For a local checkout, for
example, use `inputs.vinput.url = "path:/absolute/path/to/vinput";` in your
system flake. Then, in a NixOS module with access to `inputs`:

```nix
{ inputs, ... }: {
  imports = [ inputs.vinput.nixosModules.default ];

  programs.vinput = {
    enable = true;
    language = "en";
    # Default: http://127.0.0.1:18080/v1/audio/transcriptions
    # asrUrl = "http://127.0.0.1:18080/v1/audio/transcriptions";
  };

  # Replace alice with your user. Log out and back in after changing groups.
  users.users.alice.extraGroups = [ "ydotool" ];
}
```

The module installs the program, enables `programs.ydotool.enable`, and starts
the listener as a user service. It does **not** enable an ASR service by default:
provide an endpoint or enable `services.vinput-asr` below. If you changed
`programs.ydotool.group`, use that group instead of `ydotool`.

The flake uses `nixos-unstable`, pinned by `flake.lock`. `x86_64-linux` is the
current package target. For a Git checkout, add new files to Git before
referencing the checkout as a Git flake; Nix otherwise excludes untracked files.
`nix build path:.` includes them without staging.

A consuming flake's lock file controls its transitive inputs. If overriding
`inputs.vinput.inputs.nixpkgs` to follow the host's nixpkgs, use the same
tested revision or rebase the NPU overlay first.

### Local OpenVINO service

Enable the local service alongside `programs.vinput`. Nix downloads a pinned,
hash-verified [`OpenVINO/whisper-large-v3-turbo-int8-ov`](https://huggingface.co/OpenVINO/whisper-large-v3-turbo-int8-ov)
model by default. Its files total approximately 820 MB and require OpenVINO
2026.1 or later. Model downloads occur at build time, not when the service runs.

```nix
services.vinput-asr = {
  enable = true;
  device = "CPU";  # or "NPU" with a compatible OpenVINO stack and driver
  language = "en";
};
```

To run only the transcription service on another machine, import
`inputs.vinput.nixosModules.asr` instead of `nixosModules.default`. The latter
already imports the service module; importing both is unnecessary. Existing
`programs.vinput.localAsr` options remain aliases for the new service options.

For a different compatible model, set `services.vinput-asr.model` to a Nix
package containing the model files, or set `services.vinput-asr.modelPath` to
an existing absolute directory readable by the service user. `modelPath`
overrides `model`. The packaged default is also available as the flake package
`whisper-large-v3-turbo-int8-ov`.

The flake modules use their pinned nixpkgs for the service's OpenVINO Python
environment, even when the host uses another nixpkgs. Override
`services.vinput-asr.python` to use another compatible environment:

```nix
services.vinput-asr.python = compatiblePkgs.python3.withPackages (ps: [
  ps.openvino-genai
]);
```

The flake exposes `vinput-asr`, a standalone server executable using the
unpatched runtime; use the NixOS service or an overlay-built runtime for NPU.
With another package set, the same wrapper can be built with
`pkgs.callPackage ./nix/asr-package.nix` and a compatible `python` argument.

For Intel NPU use, select `NPU`. The flake applies a service-scoped overlay
implementing [nixpkgs PR #570230](https://github.com/NixOS/nixpkgs/pull/570230),
including the compiler, patched driver and OpenVINO library paths. It builds the
service runtime and driver from one pinned nixpkgs package set; it does not
replace the host's system-wide packages.

```nix
{
  services.vinput-asr = {
    enable = true;
    device = "NPU";
    language = "en";
  };
}
```

The module sets `ZE_ENABLE_ALT_DRIVERS` for this service; it does not install
hardware support or grant device access. Ensure the kernel driver and firmware
are enabled and the systemd service can access `/dev/accel/accel0`. Depending
on its permissions, you may need
`systemd.services.vinput-asr.serviceConfig.SupplementaryGroups`.
The host firmware must also be compatible with the selected userspace driver and
compiler. A mismatch can fail inference with `MAPPED_INFERENCE_VERSION is NOT
compatible`. On NixOS, to prefer the default patched driver's firmware over
other firmware packages, an opt-in configuration is:

```nix
{ config, lib, ... }: {
  hardware.firmware = lib.mkBefore [
    config.services.vinput-asr.npuDriver.firmware
  ];
}
```

This changes firmware system-wide, including for other NPU clients. Rebuild
the system and reboot or reload the kernel driver as appropriate; verify which
firmware is actually loaded. If overriding `npuDriver`, ensure that package has
a matching `firmware` output. This step does not guarantee that the host kernel
driver and firmware are compatible.

See the [OpenVINO NPU reference](https://github.com/ryan4yin/nix-config/pull/472).
An explicit language is required on NPU and `AUTO` because of known language
state issues; set either `programs.vinput.language` or `services.vinput-asr.language`.
The system service caches compiled models in `/var/lib/vinput-asr`; the first
NPU load can take much longer than subsequent starts.

The overlay is also exported as `overlays.openvino-npu`, and the patched driver
and compiler are available as the `intel-npu-driver` and
`openvino-npu-compiler` flake packages. Applying the overlay to a host's
`nixpkgs.overlays` is optional and affects other users of those packages.
The overlay targets the locked nixpkgs revision; review and test it when
updating either nixpkgs or the upstream PR. Automatic Intel NPU integration is
limited to `x86_64-linux`; on other systems `AUTO` uses the ordinary runtime.

For service diagnostics:

```sh
systemctl --user status vinput
journalctl --user -u vinput -f
systemctl status vinput-asr
journalctl -u vinput-asr -f
curl -fsS http://127.0.0.1:18080/health
```

## Usage

Bind `vinput toggle` to a global shortcut. Focus the target application, toggle
once to record, speak, and toggle again to transcribe and type. Alternatively:

```sh
vinput record
vinput write       # stop recording, transcribe and type
vinput status      # idle, recording, or processing
```

`stop` is an alias for `write`. Recording is capped at 30 seconds; after the cap,
invoke `write` to process the captured audio. The service reports errors in its
journal. Processing happens asynchronously; keep the intended target focused
until typing finishes. Like any virtual keyboard, ydotool types into whichever
application has focus, including terminals and password fields.

The current ydotool `type` implementation uses US keyboard mappings and does not
reliably support Unicode. vinput rejects non-ASCII transcripts rather than
typing corrupted or partial text. Newlines and tabs become spaces, so the
transcript cannot press Enter or Tab. Configure the ydotool virtual keyboard as
US layout if your desktop uses a different layout. This is batch transcription,
not realtime VAD, continuous streaming, or an assistant. Whisper can hallucinate
text on silence or noise; review dictated text before acting on it.

## Running without the NixOS module

Install `arecord` (ALSA), `curl`, `ydotool`, and a running `ydotoold` with
permissions for your user. Start `vinput listen` in your user session; the
commands above communicate through `$XDG_RUNTIME_DIR/vinput.sock`.

| Environment variable | Default | Description |
| --- | --- | --- |
| `VINPUT_ASR_URL` | `http://127.0.0.1:18080/v1/audio/transcriptions` | Transcription endpoint; audio is sent to this URL |
| `VINPUT_MODEL` | `whisper-large-v3-turbo` | Model field sent to the endpoint |
| `VINPUT_LANGUAGE` | unset | Language code, e.g. `en` |
| `VINPUT_CAPTURE_DEVICE` | ALSA default | `arecord -D` device |
| `YDOTOOL_SOCKET` | ydotool default | Socket for the ydotool daemon |

Audio is stored in unlinked, user-private files in the runtime directory during
recording and processing. The bundled ASR server listens on loopback without
authentication; do not expose it to untrusted networks. It and its API are
documented in [asr/README.md](asr/README.md).

## Development

```sh
cargo test
python3 -m unittest discover -s asr -p 'test_*.py'
python3 tests/integration.py
nix flake check
```

The integration test uses fake capture and keyboard programs and a local HTTP
server. It does not validate microphone hardware, a real model, or NPU inference.
