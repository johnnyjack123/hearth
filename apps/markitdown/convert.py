"""Converts one file to Markdown and writes it to stdout.

Runs as a child of server.py, once per file, so a crash or hang in a parser
ends only this process. Plugins stay off: they could load more converters,
some of which call web services.

A PDF with (almost) no text is a scan: its pages are rendered with poppler
(pdftoppm) and read with tesseract, in German and English. That takes
seconds per page, so pages are read as long as time allows: the answer
comes with the pages read so far, before server.py's limit ends the whole
conversion.
"""

import os
import re
import subprocess
import sys
import tempfile
import time
from concurrent.futures import ThreadPoolExecutor

from markitdown import MarkItDown

STARTED = time.monotonic()

# A PDF whose pages carry fewer characters than this on average is a scan.
MIN_CHARS_PER_PAGE = 80
# Pages read by OCR at most.
MAX_OCR_PAGES = 100
OCR_LANGUAGES = "deu+eng"
OCR_DPI = "200"
# No page starts after this many seconds of the conversion (the self-test
# sets it lower), and one still running at OCR_HARD_SECONDS is dropped:
# server.py gives the whole conversion 240 s.
OCR_SECONDS = float(os.environ.get("HEARTH_OCR_SECONDS", "180"))
OCR_HARD_SECONDS = 225
# Pages read side by side, each tesseract on one thread: one process per
# page uses the cores better than threads inside one page, and two
# conversions at a time stay within a small machine.
OCR_WORKERS = max(1, min(4, os.cpu_count() or 1) // 2)


def pdf_pages(path: str) -> int:
    """Number of pages, from pdfinfo; 0 if it cannot tell."""
    run = subprocess.run(["pdfinfo", path], capture_output=True)
    match = re.search(rb"^Pages:\s+(\d+)", run.stdout, re.MULTILINE)
    return int(match.group(1)) if match else 0


def left() -> float:
    """Seconds until a page still being read is dropped."""
    return max(1.0, OCR_HARD_SECONDS - (time.monotonic() - STARTED))


def ocr_page(path: str, work: str, number: int) -> str | None:
    """One page as tesseract reads it; None if there was no time for it."""
    if number > 1 and time.monotonic() - STARTED > OCR_SECONDS:
        return None
    image = os.path.join(work, f"page-{number}")
    try:
        subprocess.run(
            ["pdftoppm", "-r", OCR_DPI, "-gray", "-png", "-singlefile", "-f", str(number), "-l", str(number), path, image],
            check=True,
            capture_output=True,
            timeout=left(),
        )
        run = subprocess.run(
            ["tesseract", image + ".png", "stdout", "-l", OCR_LANGUAGES],
            capture_output=True,
            timeout=left(),
            env={**os.environ, "OMP_THREAD_LIMIT": "1"},
        )
    except (subprocess.TimeoutExpired, subprocess.CalledProcessError):
        return None
    return run.stdout.decode("utf-8", "replace").strip()


def ocr(path: str, pages: int) -> str:
    """The text of the first pages as tesseract reads it, as many as time allows."""
    count = min(pages, MAX_OCR_PAGES)
    with tempfile.TemporaryDirectory() as work, ThreadPoolExecutor(OCR_WORKERS) as pool:
        results = list(pool.map(lambda number: ocr_page(path, work, number), range(1, count + 1)))
    # The first pages without a gap, so "the first n pages" holds.
    texts = []
    for text in results:
        if text is None:
            break
        texts.append(text)
    if len(texts) < count:
        limit = f", only the first {len(texts)} of {pages} pages (time limit)"
    elif count < pages:
        limit = f", only the first {count} of {pages} pages"
    else:
        limit = ""
    note = f"[Text recognized by OCR from a scanned PDF; it may contain errors{limit}.]"
    body = "\n\n".join(f"## Page {number}\n\n{text}" for number, text in enumerate(texts, start=1) if text)
    return f"{note}\n\n{body}" if body else ""


def main() -> None:
    path = sys.argv[1]
    result = MarkItDown(enable_plugins=False).convert_local(path)
    # `markdown` since 0.1; `text_content` in older releases.
    text = getattr(result, "markdown", None) or getattr(result, "text_content", None) or ""
    if path.lower().endswith(".pdf"):
        pages = pdf_pages(path)
        if pages and len(re.sub(r"\s+", "", text)) < MIN_CHARS_PER_PAGE * pages:
            recognized = ocr(path, pages)
            if len(recognized) > len(text.strip()):
                text = recognized
    # Bytes, so the locale of the empty environment cannot change the encoding.
    sys.stdout.buffer.write(text.encode("utf-8"))


if __name__ == "__main__":
    main()
