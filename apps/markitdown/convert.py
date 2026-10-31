"""Converts one file to Markdown and writes it to stdout.

Runs as a child of server.py, once per file, so a crash or hang in a parser
ends only this process. Plugins stay off: they could load more converters,
some of which call web services.
"""

import sys

from markitdown import MarkItDown


def main() -> None:
    result = MarkItDown(enable_plugins=False).convert_local(sys.argv[1])
    # `markdown` since 0.1; `text_content` in older releases.
    text = getattr(result, "markdown", None) or getattr(result, "text_content", None) or ""
    # Bytes, so the locale of the empty environment cannot change the encoding.
    sys.stdout.buffer.write(text.encode("utf-8"))


if __name__ == "__main__":
    main()
