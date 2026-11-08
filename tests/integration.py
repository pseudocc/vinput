#!/usr/bin/env python3
"""Exercise the real daemon and curl with fake audio, ASR and keyboard devices."""

import email.parser
import email.policy
import http.server
import io
import os
from pathlib import Path
import subprocess
import sys
import tempfile
import threading
import time
import unittest
import wave

ROOT = Path(__file__).resolve().parents[1]
BIN = ROOT / "target/debug/vinput"


class Asr(http.server.BaseHTTPRequestHandler):
    response = b"hello\\world\nagain"
    requests = []
    hold = None
    entered = None

    def log_message(self, *_):
        pass

    def do_POST(self):
        data = self.rfile.read(int(self.headers["Content-Length"]))
        message = email.parser.BytesParser(policy=email.policy.default).parsebytes(
            b"Content-Type: " + self.headers["Content-Type"].encode() + b"\r\n\r\n" + data
        )
        parts = {p.get_param("name", header="content-disposition"): p.get_payload(decode=True)
                 for p in message.iter_parts()}
        self.requests.append((self.path, parts))
        if self.hold is not None:
            self.entered.set()
            self.hold.wait(timeout=5)
        self.send_response(200)
        self.end_headers()
        self.wfile.write(self.response)


class Integration(unittest.TestCase):
    def test_format_preview_does_not_need_daemon(self):
        for text, expected in [
            ("git clean dash dash dry dash run", "git clean --dry-run"),
            ("question mark", "?"),
            ("foo equals bar", "foo=bar"),
            ("cargo test dash dash pattern equals question mark", "cargo test --pattern=?"),
            ("tilde slash config", "~/config"),
            ("I have a question mark", "i have a question mark"),
            ("git clean\ndash dash dry dash run", "git clean dash dash dry dash run"),
            ("GIT dash capital c double dash STATUS", "git -C -- status"),
            ("capital HELLO", "Hello"),
            ("one caps lock TWO Three capslock FOUR", "one TWO THREE four"),
            ("capslock GIT caps lock status dash dash dry dash run", "GIT status --dry-run"),
        ]:
            with self.subTest(text=text):
                result = subprocess.run([BIN, "format-rules"], input=text + "\n",
                                        text=True, capture_output=True, timeout=5)
                self.assertEqual(result.returncode, 0, result.stderr)
                self.assertEqual(result.stdout, expected + "\n")
        for text in ["\x1b", "你好", "x" * 65_537]:
            result = subprocess.run([BIN, "format-rules"], input=text, text=True,
                                    capture_output=True, timeout=5)
            self.assertNotEqual(result.returncode, 0)
            self.assertEqual(result.stdout, "")

    def test_record_transcribe_type_and_shutdown(self):
        with tempfile.TemporaryDirectory() as temp:
            root = Path(temp)
            output = root / "typed"
            fake = root / "bin"
            fake.mkdir()
            for name, script in {
                "arecord": "import sys,time\nsys.stdout.buffer.write(b'\\x01\\x02'*800); sys.stdout.flush(); time.sleep(120)\n",
                "ydotool": "import os,sys\nassert sys.argv[1:] == ['type', '--file', '-']\nopen(os.environ['TYPED'], 'wb').write(sys.stdin.buffer.read())\n",
            }.items():
                path = fake / name
                path.write_text(f"#!{sys.executable}\n" + script)
                path.chmod(0o755)
            server = http.server.HTTPServer(("127.0.0.1", 0), Asr)
            worker = threading.Thread(target=server.serve_forever, daemon=True)
            worker.start()
            env = dict(os.environ, XDG_RUNTIME_DIR=temp, PATH=f"{fake}:{os.environ['PATH']}",
                       VINPUT_ASR_URL=f"http://127.0.0.1:{server.server_port}/v1/audio/transcriptions",
                       VINPUT_LANGUAGE="en", TYPED=str(output))
            daemon = subprocess.Popen([BIN, "listen"], env=env, stdout=subprocess.DEVNULL, stderr=subprocess.PIPE)

            def command(name, ok=True):
                result = subprocess.run([BIN, name], env=env, text=True, capture_output=True, timeout=5)
                self.assertEqual(result.returncode == 0, ok, result.stderr)
                return result.stdout.strip()

            try:
                for _ in range(100):
                    if (root / "vinput.sock").exists():
                        break
                    time.sleep(0.02)
                self.assertEqual(command("status"), "idle")
                self.assertEqual(command("toggle"), "recording")
                command("record", ok=False)
                time.sleep(0.1)
                self.assertEqual(command("write"), "processing")
                for _ in range(150):
                    if command("status") == "idle":
                        break
                    time.sleep(0.02)
                self.assertEqual(output.read_bytes(), b"hello\\world again")
                path, parts = Asr.requests[-1]
                self.assertEqual(path, "/v1/audio/transcriptions")
                self.assertEqual(parts["language"], b"en")
                self.assertEqual(parts["response_format"], b"text")
                with wave.open(io.BytesIO(parts["file"])) as wav:
                    self.assertEqual((wav.getnchannels(), wav.getsampwidth(), wav.getframerate()), (1, 2, 16000))
                    self.assertEqual(wav.readframes(1), b"\x01\x02")
                # The original path must not apply spoken-symbol rules.
                output.unlink()
                Asr.response = b"git clean dash dash dry dash run"
                command("record")
                command("write-rules", ok=False)
                self.assertEqual(command("status"), "recording")
                time.sleep(0.1)
                command("write")
                for _ in range(150):
                    if command("status") == "idle":
                        break
                    time.sleep(0.02)
                self.assertEqual(output.read_bytes(), Asr.response)
                # Raw mode does not consume spoken casing controls.
                output.unlink()
                Asr.response = b"MiXeD caps lock capital C"
                command("record")
                time.sleep(0.1)
                command("write")
                for _ in range(150):
                    if command("status") == "idle":
                        break
                    time.sleep(0.02)
                self.assertEqual(output.read_bytes(), Asr.response)
                for transcript, expected in [
                    (b"https colon slash slash oem dash share dot canonical dot com", b"https://oem-share.canonical.com"),
                    (b"https colon slash slash example dot com slash search question mark q equals rust ampersand page equals two", b"https://example.com/search?q=rust&page=two"),
                    (b"git clean dash dash dry dash run", b"git clean --dry-run"),
                    (b"git dash capital C double dash status", b"git -C -- status"),
                    (b"cargo test dash dash test underscore name", b"cargo test --test_name"),
                    (b"backslash", b"\\"),
                    (b"capital HELLO caps lock WORLD", b"Hello WORLD"),
                    (b"NEXT RECORDING", b"next recording"),
                ]:
                    with self.subTest(transcript=transcript):
                        output.unlink()
                        Asr.response = transcript
                        self.assertEqual(command("record-rules"), "recording")
                        command("record", ok=False)
                        command("write", ok=False)
                        command("toggle", ok=False)
                        self.assertEqual(command("status"), "recording")
                        time.sleep(0.1)
                        self.assertEqual(command("write-rules"), "processing")
                        for _ in range(150):
                            if command("status") == "idle":
                                break
                            time.sleep(0.02)
                        self.assertEqual(output.read_bytes(), expected)
                output.unlink()
                # Both modes reject starts while an ASR request is in flight.
                Asr.response = b"git clean dash dash dry dash run"
                Asr.hold = threading.Event()
                Asr.entered = threading.Event()
                try:
                    command("record-rules")
                    time.sleep(0.1)
                    command("write-rules")
                    self.assertTrue(Asr.entered.wait(timeout=5))
                    self.assertEqual(command("status"), "processing")
                    command("record", ok=False)
                    command("record-rules", ok=False)
                    self.assertFalse(output.exists())
                finally:
                    Asr.hold.set()
                for _ in range(150):
                    if command("status") == "idle":
                        break
                    time.sleep(0.02)
                self.assertEqual(output.read_bytes(), b"git clean --dry-run")
                Asr.hold = None
                Asr.entered = None
                output.unlink()
                Asr.response = "你好".encode()
                command("record-rules")
                time.sleep(0.1)
                command("write-rules")
                for _ in range(150):
                    if command("status") == "idle":
                        break
                    time.sleep(0.02)
                self.assertFalse(output.exists(), "unsupported text must not be partially typed")
                command("write", ok=False)
                command("record")
            finally:
                daemon.terminate()
                daemon.wait(timeout=5)
                daemon.stderr.close()
                server.shutdown()
                server.server_close()
                worker.join()
            self.assertFalse((root / "vinput.sock").exists())
            self.assertFalse(list(root.glob(".vinput-*")))


if __name__ == "__main__":
    subprocess.run(["cargo", "build", "--offline"], cwd=ROOT, check=True)
    unittest.main()
