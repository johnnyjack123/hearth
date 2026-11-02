"""Self-test of the converter, run in the built image without network:

    docker run --rm --network none <image> python -I /app/selftest.py

Builds a small file of each kind (Word by hand as a zip, Excel and
PowerPoint with the libraries markitdown brings, PDF by hand), sends them to
server.py over HTTP and checks the Markdown, plus the error answers.
"""

import io
import json
import os
import subprocess
import sys
import tempfile
import time
import urllib.error
import urllib.parse
import urllib.request
import zipfile

PORT = 18080
BASE = f"http://127.0.0.1:{PORT}"
HERE = os.path.dirname(os.path.abspath(__file__))


def docx(text: str) -> bytes:
    """A minimal Word document: one paragraph."""
    buffer = io.BytesIO()
    with zipfile.ZipFile(buffer, "w") as archive:
        archive.writestr(
            "[Content_Types].xml",
            '<?xml version="1.0" encoding="UTF-8" standalone="yes"?>'
            '<Types xmlns="http://schemas.openxmlformats.org/package/2006/content-types">'
            '<Default Extension="rels" ContentType="application/vnd.openxmlformats-package.relationships+xml"/>'
            '<Default Extension="xml" ContentType="application/xml"/>'
            '<Override PartName="/word/document.xml" '
            'ContentType="application/vnd.openxmlformats-officedocument.wordprocessingml.document.main+xml"/>'
            "</Types>",
        )
        archive.writestr(
            "_rels/.rels",
            '<?xml version="1.0" encoding="UTF-8" standalone="yes"?>'
            '<Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships">'
            '<Relationship Id="rId1" '
            'Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/officeDocument" '
            'Target="word/document.xml"/></Relationships>',
        )
        archive.writestr(
            "word/document.xml",
            '<?xml version="1.0" encoding="UTF-8" standalone="yes"?>'
            '<w:document xmlns:w="http://schemas.openxmlformats.org/wordprocessingml/2006/main"><w:body>'
            f"<w:p><w:r><w:t>{text}</w:t></w:r></w:p></w:body></w:document>",
        )
    return buffer.getvalue()


def xlsx() -> bytes:
    from openpyxl import Workbook

    book = Workbook()
    sheet = book.active
    sheet.title = "Preise"
    sheet.append(["Sorte", "Preis"])
    sheet.append(["Arabica", 12])
    buffer = io.BytesIO()
    book.save(buffer)
    return buffer.getvalue()


def pptx() -> bytes:
    from pptx import Presentation

    deck = Presentation()
    slide = deck.slides.add_slide(deck.slide_layouts[1])
    slide.shapes.title.text = "Röstplan"
    slide.placeholders[1].text = "Bohnen bei 200 Grad"
    buffer = io.BytesIO()
    deck.save(buffer)
    return buffer.getvalue()


def pdf(text: str) -> bytes:
    """A one-page PDF with a line of text, offsets computed for the xref table."""
    stream = f"BT /F1 24 Tf 72 720 Td ({text}) Tj ET".encode("latin-1")
    objects = [
        b"<< /Type /Catalog /Pages 2 0 R >>",
        b"<< /Type /Pages /Kids [3 0 R] /Count 1 >>",
        b"<< /Type /Page /Parent 2 0 R /MediaBox [0 0 612 792] /Resources << /Font << /F1 4 0 R >> >> /Contents 5 0 R >>",
        b"<< /Type /Font /Subtype /Type1 /BaseFont /Helvetica >>",
        b"<< /Length " + str(len(stream)).encode() + b" >>\nstream\n" + stream + b"\nendstream",
    ]
    out = bytearray(b"%PDF-1.4\n")
    offsets = []
    for number, body in enumerate(objects, start=1):
        offsets.append(len(out))
        out += f"{number} 0 obj\n".encode() + body + b"\nendobj\n"
    xref = len(out)
    out += f"xref\n0 {len(objects) + 1}\n0000000000 65535 f \n".encode()
    for offset in offsets:
        out += f"{offset:010d} 00000 n \n".encode()
    out += f"trailer\n<< /Size {len(objects) + 1} /Root 1 0 R >>\nstartxref\n{xref}\n%%EOF\n".encode()
    return bytes(out)


def jpeg_size(data: bytes) -> tuple[int, int]:
    """Width and height from the first start-of-frame marker of a JPEG."""
    position = 2
    while position < len(data):
        marker, length = data[position + 1], int.from_bytes(data[position + 2 : position + 4], "big")
        if marker in (0xC0, 0xC1, 0xC2):
            height = int.from_bytes(data[position + 5 : position + 7], "big")
            width = int.from_bytes(data[position + 7 : position + 9], "big")
            return width, height
        position += 2 + length
    raise ValueError("no frame in JPEG")


def scanned_pdf(text: str) -> bytes:
    """A PDF with the page of pdf(text) as a picture only, like a scan."""
    with tempfile.TemporaryDirectory() as work:
        source = os.path.join(work, "text.pdf")
        with open(source, "wb") as file:
            file.write(pdf(text))
        subprocess.run(["pdftoppm", "-r", "150", "-jpeg", "-singlefile", source, os.path.join(work, "scan")], check=True)
        with open(os.path.join(work, "scan.jpg"), "rb") as file:
            jpeg = file.read()
    width, height = jpeg_size(jpeg)
    content = b"q 612 0 0 792 0 0 cm /Im1 Do Q"
    objects = [
        b"<< /Type /Catalog /Pages 2 0 R >>",
        b"<< /Type /Pages /Kids [3 0 R] /Count 1 >>",
        b"<< /Type /Page /Parent 2 0 R /MediaBox [0 0 612 792] /Resources << /XObject << /Im1 4 0 R >> >> /Contents 5 0 R >>",
        f"<< /Type /XObject /Subtype /Image /Width {width} /Height {height} /ColorSpace /DeviceRGB "
        f"/BitsPerComponent 8 /Filter /DCTDecode /Length {len(jpeg)} >>\nstream\n".encode() + jpeg + b"\nendstream",
        b"<< /Length " + str(len(content)).encode() + b" >>\nstream\n" + content + b"\nendstream",
    ]
    out = bytearray(b"%PDF-1.4\n")
    offsets = []
    for number, body in enumerate(objects, start=1):
        offsets.append(len(out))
        out += f"{number} 0 obj\n".encode() + body + b"\nendobj\n"
    xref = len(out)
    out += f"xref\n0 {len(objects) + 1}\n0000000000 65535 f \n".encode()
    for offset in offsets:
        out += f"{offset:010d} 00000 n \n".encode()
    out += f"trailer\n<< /Size {len(objects) + 1} /Root 1 0 R >>\nstartxref\n{xref}\n%%EOF\n".encode()
    return bytes(out)


def post(name: str, data: bytes) -> tuple[int, dict]:
    request = urllib.request.Request(
        f"{BASE}/convert",
        data=data,
        method="POST",
        headers={"x-file-name": urllib.parse.quote(name), "content-type": "application/octet-stream"},
    )
    try:
        with urllib.request.urlopen(request, timeout=300) as response:
            return response.status, json.load(response)
    except urllib.error.HTTPError as error:
        return error.code, json.load(error)


def main() -> None:
    server = subprocess.Popen([sys.executable, "-I", os.path.join(HERE, "server.py")], env={**os.environ, "PORT": str(PORT)})
    try:
        for _ in range(50):
            try:
                with urllib.request.urlopen(f"{BASE}/health", timeout=2) as response:
                    if response.read() == b"ok":
                        break
            except OSError:
                time.sleep(0.2)
        else:
            raise SystemExit("server did not start")

        failures = []

        def expect(label: str, ok: bool, detail: object = "") -> None:
            print(("ok   " if ok else "FAIL ") + label, "" if ok else detail)
            if not ok:
                failures.append(label)

        cases = [
            ("bericht.docx", docx("Kaffee wird bei 200 Grad geröstet."), ["Kaffee wird bei 200 Grad geröstet."]),
            ("preise.xlsx", xlsx(), ["Arabica", "12", "Preise"]),
            ("plan.pptx", pptx(), ["Röstplan", "Bohnen bei 200 Grad"]),
            ("seite.pdf", pdf("Kaffee roesten"), ["Kaffee roesten"]),
            # A scan has no text layer: OCR reads the picture.
            ("scan.pdf", scanned_pdf("Kaffee roesten bei 200 Grad"), ["recognized by OCR", "Kaffee", "200"]),
            ("seite.html", "<html><body><h1>Titel</h1><p>Absatz mit <b>fett</b></p></body></html>".encode(), ["# Titel", "**fett**"]),
            ("tabelle.csv", "Sorte,Preis\nArabica,12\n".encode(), ["Arabica", "|"]),
        ]
        for name, data, wanted in cases:
            status, body = post(name, data)
            markdown = body.get("markdown", "")
            expect(f"{name} -> Markdown", status == 200 and all(word in markdown for word in wanted), (status, body))

        status, body = post("programm.exe", b"MZ")
        expect("unknown type refused", status == 415, (status, body))
        status, body = post("kaputt.pdf", b"%PDF-1.4 not really")
        expect("broken file answered", status in (200, 422), (status, body))
        # The server answers before reading 20 MB it will not use, so the
        # client may see the refusal or a broken pipe while still sending.
        try:
            status, body = post("gross.pdf", b"0" * (20 * 1024 * 1024 + 1))
            expect("too large refused", status == 413, (status, body))
        except (urllib.error.URLError, ConnectionError) as error:
            expect("too large refused", "Broken pipe" in str(error) or "reset" in str(error), error)
        if failures:
            raise SystemExit(f"{len(failures)} failed")
        print("all passed")
    finally:
        server.terminate()


if __name__ == "__main__":
    main()
