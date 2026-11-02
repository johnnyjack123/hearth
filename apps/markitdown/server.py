"""Converts documents to Markdown for the llm-harness server.

The server sends the bytes of an uploaded file (PDF, Word, Excel,
PowerPoint, HTML, EPUB ...) and gets Markdown back, which the model reads
instead of the file: it costs fewer tokens than the provider's own PDF
reading, and text-only models such as DeepSeek can read documents at all.

Parsers of office formats are attack surface, so this runs apart from the
server: in its own container without network (compose network `documents`,
internal), read-only, as an unprivileged user. Each conversion runs in a
process of its own with a time limit, so a file that hangs or crashes the
parser takes nothing else down.

API:
  GET  /health                     -> 200 "ok"
  POST /convert                    body: the file, header X-File-Name (URL-encoded)
       -> 200 {"markdown": "...", "truncated": false}
       -> 413 too large, 415 unsupported type, 422 not convertible, 504 timeout

Scanned PDFs are read with OCR (tesseract), see convert.py.
"""

import http.server
import json
import os
import resource
import subprocess
import sys
import tempfile
import threading
import urllib.parse

PORT = int(os.environ.get("PORT", "8080"))
# The server's own limit is the same; this one guards the container.
MAX_BYTES = 20 * 1024 * 1024
# Markdown returned at most; the server keeps far less for the model.
MAX_OUTPUT = 8 * 1024 * 1024
# Scanned PDFs take the longest: a few seconds per page for OCR.
TIMEOUT_SECONDS = 240
# Two conversions at a time; a third waits, so memory stays bounded.
SLOTS = threading.BoundedSemaphore(2)
SUFFIXES = {
    ".pdf",
    ".docx",
    ".pptx",
    ".xlsx",
    ".xls",
    ".epub",
    ".html",
    ".htm",
    ".csv",
    ".json",
    ".xml",
    ".txt",
    ".md",
}
CONVERT = os.path.join(os.path.dirname(os.path.abspath(__file__)), "convert.py")


def limit_cpu() -> None:
    """Runs in the child before markitdown: CPU seconds as a second fence."""
    resource.setrlimit(resource.RLIMIT_CPU, (TIMEOUT_SECONDS, TIMEOUT_SECONDS))


def convert(data: bytes, suffix: str) -> tuple[int, dict]:
    with SLOTS, tempfile.TemporaryDirectory() as work:
        path = os.path.join(work, "input" + suffix)
        with open(path, "wb") as file:
            file.write(data)
        try:
            run = subprocess.run(
                [sys.executable, "-I", CONVERT, path],
                capture_output=True,
                timeout=TIMEOUT_SECONDS,
                preexec_fn=limit_cpu,
                cwd=work,
                env={"PATH": "/usr/local/bin:/usr/bin:/bin", "HOME": work, "TMPDIR": work},
            )
        except subprocess.TimeoutExpired:
            return 504, {"error": f"conversion took longer than {TIMEOUT_SECONDS} s"}
        if run.returncode != 0:
            lines = run.stderr.decode("utf-8", "replace").strip().splitlines()
            reason = lines[-1] if lines else f"exit code {run.returncode}"
            return 422, {"error": reason[:500]}
        output = run.stdout[:MAX_OUTPUT]
        return 200, {
            "markdown": output.decode("utf-8", "replace"),
            "truncated": len(run.stdout) > MAX_OUTPUT,
        }


class Handler(http.server.BaseHTTPRequestHandler):
    server_version = "harness-markitdown"
    sys_version = ""

    def reply(self, status: int, body: dict | str) -> None:
        data = (body if isinstance(body, str) else json.dumps(body)).encode("utf-8")
        self.send_response(status)
        self.send_header("content-type", "text/plain" if isinstance(body, str) else "application/json")
        self.send_header("content-length", str(len(data)))
        self.end_headers()
        self.wfile.write(data)

    def do_GET(self) -> None:
        if self.path == "/health":
            self.reply(200, "ok")
        else:
            self.reply(404, {"error": "not found"})

    def do_POST(self) -> None:
        if self.path != "/convert":
            self.reply(404, {"error": "not found"})
            return
        try:
            length = int(self.headers.get("content-length", "-1"))
        except ValueError:
            length = -1
        if length < 0:
            self.reply(411, {"error": "content-length required"})
            return
        if length > MAX_BYTES:
            self.reply(413, {"error": f"larger than {MAX_BYTES} bytes"})
            return
        name = urllib.parse.unquote(self.headers.get("x-file-name", ""))
        suffix = os.path.splitext(name)[1].lower()
        data = self.rfile.read(length)
        if suffix not in SUFFIXES:
            self.reply(415, {"error": f"unsupported file type {suffix or '(none)'}"})
            return
        status, body = convert(data, suffix)
        self.reply(status, body)

    def log_message(self, format: str, *args) -> None:
        # One line per request on stderr, without file names (they may be private).
        sys.stderr.write(f"{self.command} {self.path} {args[1] if len(args) > 1 else ''}\n")


def main() -> None:
    server = http.server.ThreadingHTTPServer(("0.0.0.0", PORT), Handler)
    server.daemon_threads = True
    sys.stderr.write(f"markitdown listening on {PORT}\n")
    server.serve_forever()


if __name__ == "__main__":
    main()
