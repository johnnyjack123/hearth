# Document converter for the llm-harness server: markitdown behind a small
# HTTP API (server.py). Compose runs it without network, read-only, as uid
# 1000; see docker-compose.yml (service markitdown).
#
#   docker build -t harness-markitdown apps/markitdown
#   docker run --rm --network none harness-markitdown python -I /app/selftest.py
FROM docker.io/library/python:3.13-slim-trixie

ENV PYTHONDONTWRITEBYTECODE=1 \
    PYTHONUNBUFFERED=1 \
    PIP_NO_CACHE_DIR=1 \
    PIP_DISABLE_PIP_VERSION_CHECK=1

# OCR for scanned PDFs (convert.py): poppler renders the pages, tesseract
# reads them in German and English.
RUN apt-get update \
    && apt-get install -y --no-install-recommends poppler-utils tesseract-ocr tesseract-ocr-deu \
    && rm -rf /var/lib/apt/lists/*

# Only the converters for local files; the extras for audio, YouTube and
# Azure call web services and stay out.
RUN pip install "markitdown[pdf,docx,pptx,xlsx,xls]==0.1.9"

COPY server.py convert.py selftest.py /app/

USER 1000:1000
EXPOSE 8080
CMD ["python", "-I", "/app/server.py"]
