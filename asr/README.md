# Local OpenVINO Whisper service

This optional service loads one **already exported, local** OpenVINO GenAI
Whisper model. It does not download or convert models. Install `openvino-genai`
in a Python environment supported by OpenVINO and provide the device's runtime
and driver separately. No web framework, audio library, or NumPy is needed by
the service itself.

```sh
WHISPER_MODEL_DIR=/path/to/exported-whisper \
WHISPER_DEVICE=CPU \
WHISPER_LANGUAGE=en \
python3 asr/server.py
```

The model must be compatible with `openvino_genai.WhisperPipeline` and the
chosen device. Model loading happens once before the service starts listening;
`GET /health` returns `ok` once ready. The service serializes inference.

```sh
curl -fsS http://127.0.0.1:18080/health
curl -fsS http://127.0.0.1:18080/v1/audio/transcriptions \
  -F file=@recording.wav -F model=whisper-1 \
  -F language=en -F response_format=text
```

The API accepts one multipart `file`, plus optional `model`, `language`, and
`response_format` fields. `model` is accepted for OpenAI client compatibility
but **does not select or load a model**. Only `text` is supported and is the
default; the response is UTF-8 plain text. A request language overrides the
configured default; language codes are two or three ASCII letters, e.g. `en`
or `zh`. Omit both to use automatic language detection when permitted. Unknown
fields, duplicates, unsupported formats, and malformed input are rejected.

Input must be uncompressed PCM WAV, mono, 16 kHz, 8/16/24/32-bit, no longer
than 30 seconds; request bodies are limited to 2 MiB. No resampling or format
conversion is performed. `Content-Length` is required; chunked requests are not
supported. At most eight connections are handled at once, with a 15-second
socket idle timeout; excess connections are closed.

| Environment variable | Default | Description |
| --- | --- | --- |
| `WHISPER_MODEL_DIR` | required | Existing local model directory |
| `WHISPER_DEVICE` | `CPU` | OpenVINO device selector, e.g. `CPU`, `GPU`, `NPU` |
| `WHISPER_LANGUAGE` | unset | Default language code |
| `WHISPER_HOST` | `127.0.0.1` | Listening address |
| `WHISPER_PORT` | `18080` | Listening port |
| `OV_CACHE_DIR` | unset | OpenVINO compiled-model cache directory |

**NPU correctness:** for `NPU`, selectors containing `NPU`, `AUTO` selectors,
and bare `MULTI`/`HETERO` selectors, a language is required in each request
or via `WHISPER_LANGUAGE`. A missing language returns HTTP 400 without running
inference. Every inference explicitly requests the `transcribe` task. This
avoids the affected automatic-language-detection path described in
[ryan4yin/nix-config#472](https://github.com/ryan4yin/nix-config/pull/472).
Other [NPU state-reset issues](https://github.com/openvinotoolkit/openvino/issues/37937)
may still affect some models or runtimes; test your model and driver, and use
CPU if correctness is uncertain. A language must also be supported by the model.

The server is designed for local, trusted use: it has **no authentication or
TLS**. Do not expose it directly to an untrusted network. Request payloads and
transcriptions are not logged, although OpenVINO's own native diagnostics are
outside this service's control.

Tests use the standard library and a mock pipeline, not an installed model:

```sh
python3 -m unittest discover -s asr -v
```
