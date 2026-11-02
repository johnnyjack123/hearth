"""Converts one file to Markdown and writes it to stdout.

Runs as a child of server.py, once per file, so a crash or hang in a parser
ends only this process. Plugins stay off: they could load more converters,
some of which call web services.

A PDF with (almost) no text is a scan: its pages are rendered with poppler
(pdftoppm) and read with tesseract, in German and English. That takes
seconds per page, so only the first pages are read.
"""

import os
import re
import subprocess
import sys
import tempfile

from markitdown import MarkItDown

# A PDF whose pages carry fewer characters than this on average is a scan.
MIN_CHARS_PER_PAGE = 80
# Pages read by OCR at most; each takes a few seconds on a Raspberry Pi.
MAX_OCR_PAGES = 30
OCR_LANGUAGES = "deu+eng"
OCR_DPI = "200"


def pdf_pages(path: str) -> int:
    """Number of pages, from pdfinfo; 0 if it cannot tell."""
    run = subprocess.run(["pdfinfo", path], capture_output=True)
    match = re.search(rb"^Pages:\s+(\d+)", run.stdout, re.MULTILINE)
    return int(match.group(1)) if match else 0


def ocr(path: str, pages: int) -> str:
    """The text of the first pages as tesseract reads it, page by page."""
    count = min(pages, MAX_OCR_PAGES)
    with tempfile.TemporaryDirectory() as work:
        subprocess.run(
            ["pdftoppm", "-r", OCR_DPI, "-gray", "-png", "-f", "1", "-l", str(count), path, os.path.join(work, "page")],
            check=True,
            capture_output=True,
        )
        texts = []
        # pdftoppm pads the numbers, so the names sort in page order.
        for image in sorted(os.listdir(work)):
            run = subprocess.run(["tesseract", os.path.join(work, image), "stdout", "-l", OCR_LANGUAGES], capture_output=True)
            texts.append(run.stdout.decode("utf-8", "replace").strip())
    limit = f", only the first {count} of {pages} pages" if pages > count else ""
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
