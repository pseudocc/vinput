import http.client
import io
import logging
import os
import subprocess
import struct
import sys
import tempfile
import threading
import types
import unittest
from unittest import mock
import wave

import server


def wav_bytes(raw=b"\x00\x80\xff\x7f", width=2, rate=16000, channels=1):
    out = io.BytesIO()
    with wave.open(out, "wb") as writer:
        writer.setnchannels(channels)
        writer.setsampwidth(width)
        writer.setframerate(rate)
        writer.writeframes(raw)
    return out.getvalue()


def multipart(parts, boundary="sample-boundary"):
    body = bytearray()
    for name, value in parts:
        body.extend(f'--{boundary}\r\nContent-Disposition: form-data; name="{name}"'.encode())
        if name == "file":
            body.extend(b'; filename="sample.wav"\r\nContent-Type: audio/wav')
        body.extend(b"\r\n\r\n" + value + b"\r\n")
    body.extend(f"--{boundary}--\r\n".encode())
    return bytes(body)


class FakePipeline:
    def __init__(self):
        self.calls = []

    def generate(self, samples, **kwargs):
        self.calls.append((samples, kwargs))
        return "secret transcript"


class ServerTests(unittest.TestCase):
    def setUp(self):
        self.pipeline = FakePipeline()
        self.httpd = server.WhisperServer(("127.0.0.1", 0), self.pipeline)
        self.thread = threading.Thread(target=self.httpd.serve_forever, daemon=True)
        self.thread.start()

    def tearDown(self):
        self.httpd.shutdown()
        self.httpd.server_close()
        self.thread.join()

    def request(self, method="POST", path="/v1/audio/transcriptions", body=None, headers=None):
        conn = http.client.HTTPConnection(*self.httpd.server_address, timeout=5)
        try:
            conn.request(method, path, body=body, headers=headers or {})
            response = conn.getresponse()
            return response.status, response.read(), dict(response.getheaders())
        finally:
            conn.close()

    def post_parts(self, parts):
        return self.request(body=multipart(parts), headers={"Content-Type": "multipart/form-data; boundary=sample-boundary"})

    def test_transcribes_and_does_not_log_text(self):
        logged = threading.Event()
        log_info = server.LOG.info

        def capture_log(*args, **kwargs):
            log_info(*args, **kwargs)
            logged.set()

        with self.assertLogs("whisper", level=logging.INFO) as logs, \
                mock.patch.object(server.LOG, "info", side_effect=capture_log):
            status, body, headers = self.post_parts([
                ("file", wav_bytes()), ("model", b"whisper-1"),
                ("response_format", b"text"), ("language", b"FR"),
            ])
            self.assertTrue(logged.wait(2))
        self.assertEqual((status, body), (200, b"secret transcript"))
        self.assertEqual(headers["Content-Type"], "text/plain; charset=utf-8")
        self.assertNotIn("secret transcript", " ".join(logs.output))
        samples, options = self.pipeline.calls[0]
        self.assertEqual(samples, [-1.0, 32767 / 32768])
        self.assertEqual(options, {"task": "transcribe", "language": "<|fr|>"})

    def test_curl_multipart_with_stdin_wav(self):
        url = f"http://127.0.0.1:{self.httpd.server_port}/v1/audio/transcriptions"
        response = subprocess.run(
            ["curl", "--fail", "--silent", "--show-error", "--form",
             "file=@-;filename=audio.wav;type=audio/wav", "--form-string", "model=whisper-1",
             "--form-string", "language=en", "--form-string", "response_format=text", url],
            input=wav_bytes(b"\r\n\x00\x80"), capture_output=True, check=True,
        )
        self.assertEqual(response.stdout, b"secret transcript")
        self.assertEqual(self.pipeline.calls[0][0], [2573 / 32768, -1.0])

    def test_health_and_unknown_endpoint(self):
        self.assertEqual(self.request("GET", "/health")[:2], (200, b"ok\n"))
        self.assertEqual(self.request("GET", "/secret%20transcript")[0], 404)
        self.assertEqual(self.request("POST", "/bad", b"")[0], 404)

    def test_npu_requires_explicit_language_and_request_overrides_default(self):
        self.httpd.require_language = True
        data = [("file", wav_bytes())]
        status, message, _ = self.post_parts(data)
        self.assertEqual(status, 400)
        self.assertIn(b"language is required for NPU", message)
        self.assertFalse(self.pipeline.calls)
        self.httpd.default_language = "<|en|>"
        self.assertEqual(self.post_parts(data)[0], 200)
        self.assertEqual(self.pipeline.calls[-1][1]["language"], "<|en|>")
        self.assertEqual(self.post_parts(data + [("language", b"de")])[0], 200)
        self.assertEqual(self.pipeline.calls[-1][1]["language"], "<|de|>")

    def test_bad_multipart_and_fields(self):
        valid = [("file", wav_bytes())]
        for parts in [[], valid + valid, valid + [("model", b"")],
                      valid + [("language", b"en-US")], valid + [("language", b"")],
                      valid + [("response_format", b"json")], valid + [("prompt", b"x")]]:
            with self.subTest(parts=parts):
                self.assertEqual(self.post_parts(parts)[0], 400)
        self.assertFalse(self.pipeline.calls)
        self.assertEqual(self.request(body=b"abc", headers={"Content-Type": "text/plain"})[0], 400)
        self.assertEqual(self.request(body=b"abc", headers={"Content-Type": "multipart/form-data; boundary=x"})[0], 400)
        truncated = multipart(valid).split(b"--sample-boundary--")[0]
        self.assertEqual(self.request(body=truncated, headers={"Content-Type": "multipart/form-data; boundary=sample-boundary"})[0], 400)

    def test_http_length_and_transfer_encoding(self):
        headers = {"Content-Type": "multipart/form-data; boundary=x", "Content-Length": str(server.MAX_BODY + 1)}
        self.assertEqual(self.request(body=b"", headers=headers)[0], 413)
        headers = {"Content-Type": "multipart/form-data; boundary=x", "Transfer-Encoding": "chunked"}
        self.assertEqual(self.request(body=b"", headers=headers)[0], 400)

    def test_inference_failure_does_not_expose_exception(self):
        self.pipeline.generate = mock.Mock(side_effect=RuntimeError("private transcript"))
        with self.assertLogs("whisper", level="ERROR") as logs:
            status, body, _ = self.post_parts([("file", wav_bytes())])
        self.assertEqual(status, 500)
        self.assertNotIn(b"private transcript", body)
        self.assertNotIn("private transcript", " ".join(logs.output))

    def test_inference_serialized_and_health_remains_available(self):
        started = threading.Event()
        release = threading.Event()
        second_started = threading.Event()
        call_count = 0

        def generate(samples, **options):
            nonlocal call_count
            call_count += 1
            if call_count == 1:
                started.set()
                release.wait(5)
            else:
                second_started.set()
            return "ok"

        self.pipeline.generate = generate
        statuses = []
        def post():
            statuses.append(self.post_parts([("file", wav_bytes())])[0])

        first = threading.Thread(target=post)
        second = threading.Thread(target=post)
        first.start()
        try:
            self.assertTrue(started.wait(2))
            second.start()
            self.assertFalse(second_started.wait(0.1))
            self.assertEqual(self.request("GET", "/health")[0], 200)
        finally:
            release.set()
            first.join(5)
            if second.ident:
                second.join(5)
        self.assertEqual(sorted(statuses), [200, 200])
        self.assertTrue(second_started.is_set())


class WavTests(unittest.TestCase):
    def test_pcm_depths(self):
        for width, raw in [
            (1, b"\x00\x80\xff"),
            (2, struct.pack("<hhh", -32768, 0, 32767)),
            (3, b"\x00\x00\x80\x00\x00\x00\xff\xff\x7f"),
            (4, struct.pack("<iii", -2147483648, 0, 2147483647)),
        ]:
            with self.subTest(width=width):
                samples = server.decode_wav(wav_bytes(raw, width))
                self.assertEqual(samples[:2], [-1.0, 0.0])
                self.assertAlmostEqual(samples[2], 1 - 1 / (2 ** (8 * width - 1)))

    def test_bad_wav(self):
        valid = wav_bytes()
        invalid_bits = bytearray(valid)
        invalid_bits[34:36] = (15).to_bytes(2, "little")
        invalid_rate = bytearray(valid)
        invalid_rate[28:32] = (1).to_bytes(4, "little")
        inputs = [b"", valid[:-1], wav_bytes(rate=44100), wav_bytes(channels=2),
                  wav_bytes(raw=b""), bytes(invalid_bits), bytes(invalid_rate),
                  wav_bytes(raw=b"\0\0" * (server.MAX_FRAMES + 1))]
        for data in inputs:
            with self.subTest(length=len(data)):
                with self.assertRaises(server.RequestError):
                    server.decode_wav(data)


class StartupTests(unittest.TestCase):
    def test_pipeline_loaded_once_local_path_and_cache(self):
        constructor = mock.Mock(return_value=object())
        fake_module = types.SimpleNamespace(WhisperPipeline=constructor)
        with tempfile.TemporaryDirectory() as model_dir:
            env = {"WHISPER_MODEL_DIR": model_dir, "WHISPER_DEVICE": "AUTO", "WHISPER_LANGUAGE": "EN", "OV_CACHE_DIR": "/cache"}
            with mock.patch.dict(os.environ, env, clear=True), mock.patch.dict(sys.modules, {"openvino_genai": fake_module}), mock.patch.object(server, "WhisperServer") as httpd:
                server.main()
            constructor.assert_called_once_with(model_dir, "AUTO", CACHE_DIR="/cache")
            httpd.assert_called_once_with(("127.0.0.1", 18080), constructor.return_value, "<|en|>", True)

    def test_no_model_dir_fails_without_runtime(self):
        with mock.patch.dict(os.environ, {}, clear=True):
            with self.assertRaises(server.ConfigurationError):
                server.main()


if __name__ == "__main__":
    unittest.main()
