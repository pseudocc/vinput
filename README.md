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

The existing flake follows `/etc/nixos` for nixpkgs, so that path must contain a
flake on the machine evaluating it. `x86_64-linux` is the current package target.
For a Git checkout, add the new files to Git before referencing the checkout as
a Git flake; Nix otherwise excludes untracked files. `nix build path:.` includes
them without staging.

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

The service needs `python3Packages.openvino-genai` in NixOS `pkgs`. If it is
unavailable, set `services.vinput-asr.python` to a Python environment containing
`openvino_genai` from a compatible nixpkgs revision, for example:

```nix
services.vinput-asr.python = compatiblePkgs.python3.withPackages (ps: [
  ps.openvino-genai
]);
```

When the flake's nixpkgs provides `openvino-genai`, it also exposes the
`vinput-asr` package, a standalone server executable. With another package set,
the same wrapper can be built with `pkgs.callPackage ./nix/asr-package.nix` and
the `python` argument set to a compatible environment.

For Intel NPU use, [nixpkgs PR #570230](https://github.com/NixOS/nixpkgs/pull/570230)
provides a matching NPU compiler and patched driver, but it is not included or
enabled automatically. One previously tested snapshot can be added to your
system flake as:

```nix
inputs.nixpkgs-vinput-npu.url = "github:ryan4yin/nixpkgs/2ddccb854d89eb027e09198368073fdb9cde4d1d";
```

Then configure the service using the matching Python environment and driver:

```nix
{ inputs, pkgs, ... }:
let
  npu = import inputs.nixpkgs-vinput-npu { system = pkgs.stdenv.hostPlatform.system; };
in {
  services.vinput-asr = {
    enable = true;
    device = "NPU";
    language = "en";
    python = npu.python3.withPackages (ps: [ ps.openvino-genai ]);
    npuDriver = npu.intel-npu-driver;
  };
}
```

`npuDriver` sets `ZE_ENABLE_ALT_DRIVERS`; it does not install hardware support
or grant device access. Ensure the kernel driver and firmware are enabled and
the systemd service can access `/dev/accel/accel0`; depending on its permissions,
you may need `systemd.services.vinput-asr.serviceConfig.SupplementaryGroups`.
See the [OpenVINO NPU reference](https://github.com/ryan4yin/nix-config/pull/472).
An explicit language is required on NPU and `AUTO` because of known language
state issues; set either `programs.vinput.language` or `services.vinput-asr.language`.
The system service caches compiled models in `/var/lib/vinput-asr`; the first
NPU load can take much longer than subsequent starts.

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
nix eval --impure --expr 'import ./tests/nix.nix { nixpkgs = (builtins.getFlake (toString /etc/nixos)).inputs.nixpkgs; }'
```

The integration test uses fake capture and keyboard programs and a local HTTP
server. It does not validate microphone hardware, a real model, or NPU inference.
