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
provide an endpoint or enable `localAsr` below. If you changed
`programs.ydotool.group`, use that group instead of `ydotool`.

The existing flake follows `/etc/nixos` for nixpkgs, so that path must contain a
flake on the machine evaluating it. `x86_64-linux` is the current package target.
For a Git checkout, add the new files to Git before referencing the checkout as
a Git flake; Nix otherwise excludes untracked files. `nix build path:.` includes
them without staging.

### Local OpenVINO service

Download an OpenVINO GenAI-compatible Whisper model, such as
[`OpenVINO/whisper-large-v3-turbo-int8-ov`](https://huggingface.co/OpenVINO/whisper-large-v3-turbo-int8-ov),
and place **all** its files in a directory readable by the system service. For
example, use the Hugging Face CLI's `hf download` with `--local-dir`, then copy
the resulting directory to `/var/lib/vinput-model`. Model downloads are not
performed automatically or pinned by this project.

```sh
hf download OpenVINO/whisper-large-v3-turbo-int8-ov --local-dir "$HOME/vinput-model"
sudo mkdir -p /var/lib/vinput-model
sudo cp -r "$HOME/vinput-model/." /var/lib/vinput-model/
sudo chmod -R a+rX /var/lib/vinput-model
```

```nix
programs.vinput.localAsr = {
  enable = true;
  modelPath = "/var/lib/vinput-model";
  device = "CPU";  # or "NPU" with a compatible OpenVINO stack and driver
  language = "en";
};
```

This requires `python3Packages.openvino-genai` in the NixOS `pkgs`. If it is
unavailable, set `localAsr.python` to a Python environment containing
`openvino_genai` from a compatible nixpkgs revision, for example:

```nix
programs.vinput.localAsr.python = compatiblePkgs.python3.withPackages (ps: [
  ps.openvino-genai
  ps.numpy
]);
```

For Intel NPU use, [nixpkgs PR #570230](https://github.com/NixOS/nixpkgs/pull/570230)
provides a matching NPU compiler and patched driver, but it is not included or
enabled automatically. Pin a compatible nixpkgs revision, install/configure the
matching driver and OpenVINO runtime, and ensure the service can access the NPU.
Depending on your setup you may need `ZE_ENABLE_ALT_DRIVERS` and an appropriate
device group in `systemd.services.vinput-asr`. See the
[working OpenVINO NPU reference](https://github.com/ryan4yin/nix-config/pull/472).
An explicit language is required on NPU and `AUTO` because of known language
state issues; set either `programs.vinput.language` or `localAsr.language`.
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
```

The integration test uses fake capture and keyboard programs and a local HTTP
server. It does not validate microphone hardware, a real model, or NPU inference.
