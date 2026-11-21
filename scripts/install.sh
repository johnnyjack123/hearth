#!/bin/sh
# Installs hearth on Linux or macOS for the current user, without root:
#
#   curl -fsSL https://github.com/johnnyjack123/hearth/releases/latest/download/install.sh | sh
#   sh install.sh [archive or folder]   # a downloaded release, offline
#
# Linux: hearth, hearthd and hearth-app go to ~/.local/bin (HEARTH_PREFIX
# changes that). macOS: Hearth.app goes to ~/Applications, and links to its
# programs to ~/.local/bin. A running background service is stopped first
# and started again by `hearth setup`, which runs at the end
# (HEARTH_NO_SETUP=1 skips it).
set -eu

REPOSITORY=johnnyjack123/hearth
case "${LANG:-}${LC_ALL:-}" in
  *de_*) GERMAN=1 ;;
  *) GERMAN= ;;
esac
say() { # say <German> <English>
  if [ -n "$GERMAN" ]; then printf '%s\n' "$1"; else printf '%s\n' "$2"; fi
}
die() {
  say "Fehler: $1" "Error: $2" >&2
  exit 1
}

case "$(uname -s)" in
  Linux) OS=linux ;;
  Darwin) OS=macos ;;
  *) die "nur Linux und macOS; für Windows gibt es hearth-setup.exe" "Linux and macOS only; Windows has hearth-setup.exe" ;;
esac
case "$(uname -m)" in
  x86_64 | amd64) ARCH=x64 ;;
  aarch64 | arm64) ARCH=arm64 ;;
  *) die "Prozessor $(uname -m) wird nicht unterstützt" "processor $(uname -m) is not supported" ;;
esac
# macOS gets one archive for both processors.
if [ "$OS" = macos ]; then
  NAME=hearth-macos
else
  NAME="hearth-$OS-$ARCH"
fi

WORK=$(mktemp -d)
trap 'rm -rf "$WORK"' EXIT

SOURCE="${1:-}"
if [ -z "$SOURCE" ]; then
  URL="https://github.com/$REPOSITORY/releases/latest/download/$NAME.tar.gz"
  say "Lade $URL …" "Downloading $URL …"
  if command -v curl > /dev/null 2>&1; then
    curl -fsSL "$URL" -o "$WORK/$NAME.tar.gz" || die "Download gescheitert" "download failed"
  elif command -v wget > /dev/null 2>&1; then
    wget -qO "$WORK/$NAME.tar.gz" "$URL" || die "Download gescheitert" "download failed"
  else
    die "weder curl noch wget gefunden" "neither curl nor wget found"
  fi
  SOURCE="$WORK/$NAME.tar.gz"
fi
if [ -d "$SOURCE" ]; then
  FILES="$SOURCE"
else
  mkdir -p "$WORK/unpacked"
  tar -xzf "$SOURCE" -C "$WORK/unpacked" || die "$SOURCE ist kein Release-Archiv" "$SOURCE is no release archive"
  FILES="$WORK/unpacked"
fi

BIN="${HEARTH_PREFIX:-$HOME/.local/bin}"
mkdir -p "$BIN"

# The service holds its programs open; setup starts it again.
if [ -x "$BIN/hearth" ]; then
  "$BIN/hearth" daemon stop > /dev/null 2>&1 || true
fi

if [ "$OS" = macos ]; then
  [ -d "$FILES/Hearth.app" ] || die "Hearth.app fehlt im Archiv" "Hearth.app is missing in the archive"
  mkdir -p "$HOME/Applications"
  rm -rf "$HOME/Applications/Hearth.app"
  cp -R "$FILES/Hearth.app" "$HOME/Applications/"
  # Downloaded, not signed: without this, Gatekeeper refuses to start it.
  xattr -dr com.apple.quarantine "$HOME/Applications/Hearth.app" 2> /dev/null || true
  for program in hearth hearthd hearth-app; do
    ln -sf "$HOME/Applications/Hearth.app/Contents/MacOS/$program" "$BIN/$program"
  done
  say "Hearth.app in ~/Applications, Befehle in $BIN." "Hearth.app in ~/Applications, commands in $BIN."
else
  for program in hearth hearthd hearth-app; do
    [ -f "$FILES/$program" ] || die "$program fehlt im Archiv" "$program is missing in the archive"
    # A running program keeps its old file; the new one takes its name.
    cp "$FILES/$program" "$BIN/.$program.new"
    chmod 755 "$BIN/.$program.new"
    mv -f "$BIN/.$program.new" "$BIN/$program"
  done
  say "hearth, hearthd und hearth-app in $BIN." "hearth, hearthd and hearth-app in $BIN."
fi

case ":$PATH:" in
  *":$BIN:"*) ;;
  *) say "Hinweis: $BIN steht nicht im PATH; in der Shell-Konfiguration ergänzen." \
    "Note: $BIN is not on the PATH; add it in the shell's configuration." ;;
esac

if [ -z "${HEARTH_NO_SETUP:-}" ]; then
  # setup asks questions; through `curl | sh` stdin is the script, so the
  # terminal answers.
  if [ -r /dev/tty ]; then
    "$BIN/hearth" setup < /dev/tty
  else
    "$BIN/hearth" setup
  fi
fi
