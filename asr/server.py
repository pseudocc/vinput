#!/usr/bin/env python3
"""Local, single-model OpenAI-compatible Whisper transcription endpoint."""

import array
from email import policy
from email.parser import BytesHeaderParser, BytesParser
import io
import logging
import os
from pathlib import Path
import re
import socket
import struct
import sys
import threading
import wave
from http.server import BaseHTTPRequestHandler, ThreadingHTTPServer


MAX_BODY = 2 * 1024 * 1024
MAX_FRAMES = 30 * 16000
MAX_PARTS = 4
LANGUAGE_RE = re.compile(r"[a-zA-Z]{2,3}\Z")
BOUNDARY_RE = re.compile(r"[0-9A-Za-z'()+_,./:=?-]{1,70}\Z")
LOG = logging.getLogger("whisper")


class RequestError(Exception):
    def __init__(self, status, message):
        super().__init__(message)
        self.status = status
        self.message = message


class ConfigurationError(Exception):
    pass


def language_token(value):
    if not LANGUAGE_RE.fullmatch(value):
        raise ValueError("language must be a two- or three-letter code")
    return "<|" + value.lower() + "|>"


def parse_multipart(content_type, body):
    # Parse the Content-Type separately; never interpolate unvalidated headers into
    # a synthetic message (which would permit header injection).
    header = BytesHeaderParser(policy=policy.default).parsebytes(
        b"Content-Type: " + content_type.encode("ascii") + b"\r\n\r\n"
    )
    if header.defects or header["Content-Type"].defects or header.get_content_type() != "multipart/form-data":
        raise RequestError(400, "Content-Type must be multipart/form-data")
    boundary = header.get_boundary()
    if boundary is None or not BOUNDARY_RE.fullmatch(boundary):
        raise RequestError(400, "invalid multipart boundary")
    marker = b"--" + boundary.encode("ascii")
    # Bound parser allocation, including malformed inputs with many empty parts.
    delimiter = re.compile(rb"(?m)^" + re.escape(marker) + rb"(?:--)?[ \t]*\r?$")
    if sum(1 for _ in delimiter.finditer(body)) > MAX_PARTS + 1:
        raise RequestError(400, "too many multipart parts")
    message = BytesParser(policy=policy.default).parsebytes(
        b"Content-Type: " + content_type.encode("ascii") + b"\r\nMIME-Version: 1.0\r\n\r\n" + body
    )
    if message.defects or not message.is_multipart():
        raise RequestError(400, "malformed multipart body")
    if (message.preamble and message.preamble.strip()) or (message.epilogue and message.epilogue.strip()):
        raise RequestError(400, "malformed multipart body")
    parts = message.get_payload()
    if not 1 <= len(parts) <= MAX_PARTS:
        raise RequestError(400, "invalid number of multipart parts")
    fields = {}
    for part in parts:
        if part.defects or part.is_multipart() or len(part.get_all("Content-Disposition", [])) != 1:
            raise RequestError(400, "malformed multipart part")
        disposition = part["Content-Disposition"]
        if disposition.defects or part.get_content_disposition() != "form-data":
            raise RequestError(400, "invalid multipart disposition")
        name = part.get_param("name", header="content-disposition")
        if name not in {"file", "model", "language", "response_format"} or name in fields:
            raise RequestError(400, "unknown or duplicate multipart field")
        transfer = part.get("Content-Transfer-Encoding", "binary").lower()
        if transfer not in {"binary", "8bit", "7bit"}:
            raise RequestError(400, "unsupported content transfer encoding")
        payload = part.get_payload(decode=True)
        if payload is None:
            raise RequestError(400, "invalid multipart payload")
        if name != "file":
            if len(payload) > 256 or part.get_filename() is not None:
                raise RequestError(400, "invalid multipart field")
            try:
                fields[name] = payload.decode("utf-8").strip()
            except UnicodeDecodeError:
                raise RequestError(400, "invalid multipart field") from None
        else:
            fields[name] = payload
    if "file" not in fields:
        raise RequestError(400, "file is required")
    if "model" in fields and not fields["model"]:
        raise RequestError(400, "model cannot be empty")
    if fields.get("response_format", "text") != "text":
        raise RequestError(400, "only response_format=text is supported")
    if "language" in fields:
        try:
            fields["language"] = language_token(fields["language"])
        except ValueError:
            raise RequestError(400, "invalid language") from None
    return fields


def decode_wav(data):
    # Validate the RIFF envelope and data chunk in addition to wave's checks;
    # wave otherwise silently accepts some truncated or partial PCM data.
    if len(data) < 12 or data[:4] != b"RIFF" or data[8:12] != b"WAVE":
        raise RequestError(400, "invalid WAV")
    if int.from_bytes(data[4:8], "little") + 8 != len(data):
        raise RequestError(400, "invalid WAV length")
    pos, data_sizes, formats = 12, [], []
    while pos < len(data):
        if pos + 8 > len(data):
            raise RequestError(400, "invalid WAV chunk")
        tag = data[pos:pos + 4]
        size = int.from_bytes(data[pos + 4:pos + 8], "little")
        pos += 8
        if size > len(data) - pos:
            raise RequestError(400, "truncated WAV chunk")
        if tag == b"data":
            data_sizes.append(size)
        elif tag == b"fmt ":
            formats.append(data[pos:pos + size])
        pos += size
        if size % 2 and pos < len(data):
            pos += 1
    if len(data_sizes) != 1 or len(formats) != 1 or len(formats[0]) < 16:
        raise RequestError(400, "WAV must contain one format and one data chunk")
    fmt = formats[0]
    format_tag, channels, rate, byte_rate, alignment, bits = struct.unpack_from("<HHIIHH", fmt)
    if (format_tag not in (1, 0xFFFE) or channels != 1 or rate != 16000
            or bits not in (8, 16, 24, 32) or alignment != bits // 8
            or byte_rate != rate * alignment):
        raise RequestError(400, "WAV must be 16 kHz mono uncompressed PCM")
    if format_tag == 0xFFFE and (len(fmt) < 40 or int.from_bytes(fmt[18:20], "little") != bits):
        raise RequestError(400, "unsupported WAV sample precision")
    try:
        with wave.open(io.BytesIO(data), "rb") as wav:
            width = wav.getsampwidth()
            if wav.getnchannels() != 1 or wav.getframerate() != 16000 or wav.getcomptype() != "NONE" or width not in (1, 2, 3, 4):
                raise RequestError(400, "WAV must be 16 kHz mono uncompressed PCM")
            frames = wav.getnframes()
            if not frames or data_sizes[0] % width:
                raise RequestError(400, "invalid WAV sample data")
            if frames > MAX_FRAMES:
                raise RequestError(413, "audio exceeds 30 seconds")
            raw = wav.readframes(frames)
            if len(raw) != frames * width:
                raise RequestError(400, "truncated WAV sample data")
    except (wave.Error, EOFError, ValueError):
        raise RequestError(400, "invalid WAV") from None

    if width == 1:
        return [(sample - 128) / 128.0 for sample in raw]
    if width == 3:
        return [int.from_bytes(raw[i:i + 3], "little", signed=True) / 8388608.0 for i in range(0, len(raw), 3)]
    samples = array.array("h" if width == 2 else "i")
    if samples.itemsize != width:
        raise RuntimeError("unsupported platform sample size")
    samples.frombytes(raw)
    if sys.byteorder != "little":
        samples.byteswap()
    scale = float(1 << (width * 8 - 1))
    return [sample / scale for sample in samples]


class WhisperServer(ThreadingHTTPServer):
    daemon_threads = True

    def __init__(self, address, pipeline, default_language=None, require_language=False):
        super().__init__(address, Handler)
        self.pipeline = pipeline
        self.default_language = default_language
        self.require_language = require_language
        self.inference_lock = threading.Lock()
        self.slots = threading.BoundedSemaphore(8)

    def get_request(self):
        sock, address = super().get_request()
        sock.settimeout(15)
        return sock, address

    def process_request(self, request, client_address):
        if not self.slots.acquire(blocking=False):
            self.shutdown_request(request)
            return
        try:
            super().process_request(request, client_address)
        except BaseException:
            self.slots.release()
            raise

    def process_request_thread(self, request, client_address):
        try:
            super().process_request_thread(request, client_address)
        finally:
            self.slots.release()

    def handle_error(self, request, client_address):
        # Exception messages and request data may contain sensitive content.
        LOG.error("request handler failed")


class Handler(BaseHTTPRequestHandler):
    server_version = "LocalWhisper"
    sys_version = ""

    def log_message(self, format, *args):
        # Default logging includes the arbitrary request target.
        pass

    def respond(self, status, body, content_type="text/plain; charset=utf-8"):
        payload = body.encode("utf-8")
        self.send_response(status)
        self.send_header("Content-Type", content_type)
        self.send_header("Content-Length", str(len(payload)))
        self.send_header("Cache-Control", "no-store")
        self.send_header("Connection", "close")
        self.end_headers()
        self.wfile.write(payload)
        self.close_connection = True

    def do_GET(self):
        if self.path == "/health":
            self.respond(200, "ok\n")
        else:
            self.respond(404, "not found\n")

    def do_POST(self):
        status = 500
        try:
            if self.path != "/v1/audio/transcriptions":
                raise RequestError(404, "not found")
            if self.headers.get_all("Transfer-Encoding"):
                raise RequestError(400, "transfer encoding is not supported")
            lengths = self.headers.get_all("Content-Length", [])
            if len(lengths) != 1 or not re.fullmatch(r"[0-9]{1,10}", lengths[0]):
                raise RequestError(400, "a valid Content-Length is required")
            length = int(lengths[0])
            if length > MAX_BODY:
                raise RequestError(413, "request body too large")
            types = self.headers.get_all("Content-Type", [])
            if len(types) != 1 or len(types[0]) > 256 or not types[0].isascii() or any(ord(c) < 32 or ord(c) == 127 for c in types[0]):
                raise RequestError(400, "invalid Content-Type")
            body = self.rfile.read(length)
            if len(body) != length:
                raise RequestError(400, "incomplete request body")
            fields = parse_multipart(types[0], body)
            language = fields.get("language", self.server.default_language)
            if self.server.require_language and not language:
                raise RequestError(400, "language is required for NPU; set WHISPER_LANGUAGE or include language")
            samples = decode_wav(fields["file"])
            options = {"task": "transcribe"}
            if language:
                options["language"] = language
            with self.server.inference_lock:
                result = self.server.pipeline.generate(samples, **options)
            status = 200
            self.respond(status, str(result))
        except RequestError as error:
            status = error.status
            self.respond(status, error.message + "\n")
        except (socket.timeout, TimeoutError):
            status = 408
            try:
                self.respond(status, "request timed out\n")
            except OSError:
                pass
        except (BrokenPipeError, ConnectionResetError):
            pass
        except Exception:
            LOG.error("transcription failed")
            try:
                self.respond(500, "transcription failed\n")
            except OSError:
                pass
        finally:
            LOG.info("transcription request: status %d", status)


def main():
    logging.basicConfig(level=logging.INFO, format="%(levelname)s %(message)s")
    model_dir = os.environ.get("WHISPER_MODEL_DIR", "")
    if not model_dir or not Path(model_dir).is_dir():
        raise ConfigurationError("WHISPER_MODEL_DIR must name an existing local directory")
    device = os.environ.get("WHISPER_DEVICE", "CPU")
    host = os.environ.get("WHISPER_HOST", "127.0.0.1")
    try:
        port = int(os.environ.get("WHISPER_PORT", "18080"))
    except ValueError:
        raise ConfigurationError("WHISPER_PORT must be between 1 and 65535") from None
    if not 1 <= port <= 65535:
        raise ConfigurationError("WHISPER_PORT must be between 1 and 65535")
    language = os.environ.get("WHISPER_LANGUAGE", "")
    try:
        default_language = language_token(language) if language else None
    except ValueError:
        raise ConfigurationError("WHISPER_LANGUAGE must be a two- or three-letter code") from None
    cache_dir = os.environ.get("OV_CACHE_DIR", "")
    import openvino_genai

    LOG.info("loading local Whisper pipeline")
    options = {"CACHE_DIR": cache_dir} if cache_dir else {}
    pipeline = openvino_genai.WhisperPipeline(str(Path(model_dir).resolve()), device, **options)
    # AUTO may select an NPU even without explicit device priorities.
    # Treat unspecified MULTI/HETERO priorities conservatively as well.
    selector = device.upper().strip()
    require_language = "NPU" in selector or selector.startswith("AUTO") or selector in {"MULTI", "HETERO"}
    with WhisperServer((host, port), pipeline, default_language, require_language) as server:
        LOG.info("Whisper service listening on %s:%d", host, port)
        try:
            server.serve_forever()
        except KeyboardInterrupt:
            pass


if __name__ == "__main__":
    try:
        main()
    except ConfigurationError as error:
        LOG.error("%s", error)
        sys.exit(1)
    except Exception as error:
        # Startup errors occur before the server receives any audio or transcripts.
        LOG.error("could not start Whisper service: %s: %s", type(error).__name__, error)
        sys.exit(1)
