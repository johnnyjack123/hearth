# Helpers for app recipes (crates/hearth-core/defaults/apps/*.toml).
#
# hearth runs this file and the recipe's `install` as one script with `sh`
# in a short-lived container: the base image, as the agent user, root
# filesystem read-only, network through the project's gateway (the recipe's
# `hosts` are granted for the install). It provides:
#
#   PREFIX   the app's folder, empty and writable. Instances mount it at the
#            same path, so absolute paths written by the install stay valid.
#   WORK     scratch folder on the VM's disk (unlike /tmp, which is RAM),
#            removed afterwards
#   MACHINE  x86_64 | aarch64 (uname -m)
#   ARCH     amd64 | arm64 (Debian's and Go's spelling)
#
# Apps the recipe `requires` are mounted read-only and on PATH.

set -eu

MACHINE=$(uname -m)
case "$MACHINE" in
  x86_64) ARCH=amd64 ;;
  aarch64) ARCH=arm64 ;;
  *) echo "Unbekannte Architektur: $MACHINE" >&2; exit 1 ;;
esac
mkdir -p "$WORK"

# Downloads URL $1 to file $2, HTTPS only, failing on HTTP errors.
hearth_download() {
  curl -fsSL --retry 3 --proto '=https' --proto-redir '=https' -o "$2" "$1"
}

# Fails unless file $1 has the SHA-256 checksum $2.
hearth_sha256() {
  echo "$2  $1" | sha256sum -c --quiet -
}

# Records the installed version for `hearth app list` and the browser.
# Called as hearth_version "$(program --version)": a program that is
# missing or fails only leaves the argument empty, so that fails here.
hearth_version() {
  if [ -z "${1:-}" ]; then
    echo "Version nicht ermittelbar: Das installierte Programm startet nicht." >&2
    exit 1
  fi
  printf '%s\n' "$1" > "$HEARTH_VERSION_FILE"
}

# apt without root rights and without touching the system: lists and
# downloads stay in $WORK. The image's apt.conf.d is left out: Debian's
# images clean /var/cache/apt there after every update, which fails without
# root. apt reads that folder before any -o option, so only APT_CONFIG can
# point it elsewhere; the folder's two other settings come back as options.
_hearth_apt_get() {
  APT_CONFIG="$WORK/apt/apt.conf" apt-get -q -o "Dir::State::Lists=$WORK/apt/lists" \
    -o "Dir::Cache=$WORK/apt/cache" -o Debug::NoLocking=1 -o "APT::Sandbox::User=$(id -un)" \
    -o Acquire::Languages=none -o Acquire::GzipIndexes=true "$@"
}

# Unpacks Debian packages, with the dependencies the base image lacks, into
# $PREFIX/root. Packages the base image has stay out even if the archive has
# a newer version: two versions of one library side by side would be worse
# than headers of a newer point release.
hearth_apt() {
  mkdir -p "$WORK/apt/conf.d" "$WORK/apt/lists/partial" "$WORK/apt/cache/archives/partial" \
    "$WORK/apt/debs" "$PREFIX/root"
  echo "Dir::Etc::Parts \"$WORK/apt/conf.d\";" > "$WORK/apt/apt.conf"
  _hearth_apt_get update -qq
  # Separately, so a package apt does not know fails here instead of
  # vanishing in the pipe below.
  if ! simulation=$(_hearth_apt_get install -s --no-install-recommends "$@"); then
    echo "apt kann nicht installieren: $*" >&2
    exit 1
  fi
  # "Inst name (version …)" is a new package, "Inst name [old] (new …)" an
  # upgrade of one the base image has.
  packages=$(printf '%s\n' "$simulation" | sed -n 's/^Inst \([^ ]*\) (.*/\1/p')
  if [ -z "$packages" ]; then
    echo "Schon im Base-Image: $*"
    return 0
  fi
  echo "Pakete: $(echo $packages)"
  # A .deb given as a path (one the user approved) is no download; apt only
  # brings its dependencies. It is unpacked like the downloaded ones.
  own=""
  for argument in "$@"; do
    case "$argument" in
      */*.deb)
        own="$own $(dpkg-deb --show --showformat='${Package}' "$argument")"
        cp "$argument" "$WORK/apt/debs/"
        ;;
    esac
  done
  downloads=""
  for package in $packages; do
    case " $own " in
      *" $package "*) ;;
      *) downloads="$downloads $package" ;;
    esac
  done
  if [ -n "$downloads" ]; then
    # shellcheck disable=SC2086 # one word per package
    (cd "$WORK/apt/debs" && _hearth_apt_get download $downloads)
  fi
  for deb in "$WORK/apt/debs"/*.deb; do
    dpkg-deb -x "$deb" "$PREFIX/root"
    # shellcheck disable=SC2016 # dpkg's format, not the shell's
    dpkg-deb --show --showformat='${Package} ${Version}\n' "$deb" >> "$WORK/apt/unpacked"
  done
  rm -rf "$WORK/apt/debs"
}

# Version of the Debian package $1: as hearth_apt unpacked it, else as the
# base image has it.
hearth_deb_version() {
  awk -v package="$1" '$1 == package { print $2; exit }' "$WORK/apt/unpacked" 2>/dev/null \
    | grep . || dpkg-query -W -f='${Version}' "$1" 2>/dev/null
}

# npm packages with their commands into $PREFIX (lib/node_modules, bin).
# Their install scripts run here in the installer, as in any project.
hearth_npm() {
  npm_config_cache="$WORK/npm" npm install --global --prefix "$PREFIX" --no-fund --no-audit \
    -- "$@"
}

# Version of the npm package $1 that hearth_npm installed.
hearth_npm_version() {
  node -p 'require(process.argv[1]).version' "$PREFIX/lib/node_modules/$1/package.json"
}

# Python programs, each in an environment of its own below $PREFIX/tools
# with its commands in $PREFIX/bin (like pipx). They run on the base image's
# Python, so uv downloads none.
hearth_pip() {
  # $PREFIX/bin on PATH only spares uv's hint to put it there.
  for package in "$@"; do
    PATH="$PREFIX/bin:$PATH" UV_TOOL_DIR="$PREFIX/tools" UV_TOOL_BIN_DIR="$PREFIX/bin" \
      UV_CACHE_DIR="$WORK/uv" UV_PYTHON_DOWNLOADS=never \
      uv tool install --python /usr/bin/python3 "$package"
  done
}

# Version of the Python program $1 (normalized name) that hearth_pip installed.
hearth_pip_version() {
  "$PREFIX/tools/$1/bin/python" -c \
    'import sys, importlib.metadata as m; print(m.version(sys.argv[1]))' "$1"
}

# Library folders of the packages hearth_apt unpacked.
hearth_libs() {
  echo "$PREFIX/root/usr/lib/$MACHINE-linux-gnu:$PREFIX/root/usr/lib"
}

# Writes a wrapper to $PREFIX/bin for every program in the given folders
# below $PREFIX/root (default usr/bin). Each finds the unpacked libraries
# through LD_LIBRARY_PATH, set only for it and what it starts.
hearth_wrap() {
  [ $# -gt 0 ] || set -- usr/bin
  mkdir -p "$PREFIX/bin"
  for dir in "$@"; do
    for program in "$PREFIX/root/$dir"/*; do
      [ -f "$program" ] && [ -x "$program" ] || continue
      hearth_wrapper "${program##*/}" "$program"
    done
  done
}

# Writes the wrapper $PREFIX/bin/$1 for program $2; further arguments go
# before the caller's.
hearth_wrapper() {
  name=$1
  program=$2
  shift 2
  {
    echo '#!/bin/sh'
    printf 'LD_LIBRARY_PATH=%s${LD_LIBRARY_PATH:+:$LD_LIBRARY_PATH} exec %s' \
      "$(hearth_libs)" "$program"
    for argument in "$@"; do
      printf ' %s' "$argument"
    done
    echo ' "$@"'
  } > "$PREFIX/bin/$name"
  chmod 755 "$PREFIX/bin/$name"
}

# Program files the user approved on the computer ($HEARTH_FILE, see
# hearth-core/src/app_file.rs).

# The program as command $1.
hearth_program() {
  mkdir -p "$PREFIX/bin"
  cp "$HEARTH_FILE" "$PREFIX/bin/$1"
  chmod 755 "$PREFIX/bin/$1"
}

# The AppImage unpacked, with command $1 starting it: instances have no
# FUSE to mount it, and unpacking on every start would be slow.
hearth_appimage() {
  cp "$HEARTH_FILE" "$WORK/app.AppImage"
  chmod 755 "$WORK/app.AppImage"
  (cd "$WORK" && ./app.AppImage --appimage-extract > /dev/null)
  mv "$WORK/squashfs-root" "$PREFIX/app"
  mkdir -p "$PREFIX/bin"
  printf '#!/bin/sh\nAPPDIR=%s exec %s "$@"\n' "$PREFIX/app" "$PREFIX/app/AppRun" \
    > "$PREFIX/bin/$1"
  chmod 755 "$PREFIX/bin/$1"
}

# Whether file $1 is a program: executable, or an ELF file or script whose
# executable bit got lost (zip archives made on Windows).
_hearth_is_program() {
  [ -x "$1" ] && return 0
  case "$(od -An -tx1 -N4 "$1" | tr -d ' \n')" in
    7f454c46 | 2321*) return 0 ;;
  esac
  return 1
}

# The archive $HEARTH_FILE (tar or zip, as $1 says) unpacked to
# $PREFIX/files, its programs linked into $PREFIX/bin: those at the top or
# one folder down, and those in any bin folder.
hearth_unpack() {
  mkdir -p "$PREFIX/files" "$PREFIX/bin"
  case "$1" in
    zip) unzip -q "$HEARTH_FILE" -d "$PREFIX/files" ;;
    *) tar -xf "$HEARTH_FILE" -C "$PREFIX/files" --no-same-owner ;;
  esac
  find "$PREFIX/files" -maxdepth 4 -type f | sort > "$WORK/files"
  while IFS= read -r program; do
    relative=${program#"$PREFIX/files/"}
    folder=${program%/*}
    case "$relative" in
      */*/*) [ "${folder##*/}" = bin ] || continue ;;
    esac
    _hearth_is_program "$program" || continue
    chmod 755 "$program"
    name=${program##*/}
    [ -e "$PREFIX/bin/$name" ] || ln -s "$program" "$PREFIX/bin/$name"
  done < "$WORK/files"
  if [ -z "$(ls -A "$PREFIX/bin")" ]; then
    echo "Im Archiv ist kein Programm (oben, einen Ordner tiefer oder in einem bin-Ordner)." >&2
    exit 1
  fi
}

# Records the version program $1 (default: the first in $PREFIX/bin) names
# for --version, else "Datei". A program that does not know --version may
# start something else; ten seconds are enough to answer.
hearth_file_version() {
  program=${1:-$(find "$PREFIX/bin" -mindepth 1 -maxdepth 1 | sort | head -n 1)}
  version=$(timeout 10 "$program" --version < /dev/null 2>&1 \
    | grep -oE '[0-9]+(\.[0-9]+)+' | head -n 1) || true
  hearth_version "${version:-Datei}"
}

# The helpers' names before 0.2.0 (llm-harness), for own programs written
# with them; new scripts use the hearth_ names.
harness_download() { hearth_download "$@"; }
harness_version() { hearth_version "$@"; }
harness_sha256() { hearth_sha256 "$@"; }
harness_apt() { hearth_apt "$@"; }
harness_deb_version() { hearth_deb_version "$@"; }
harness_npm() { hearth_npm "$@"; }
harness_npm_version() { hearth_npm_version "$@"; }
harness_pip() { hearth_pip "$@"; }
harness_pip_version() { hearth_pip_version "$@"; }
harness_libs() { hearth_libs "$@"; }
harness_wrap() { hearth_wrap "$@"; }
harness_wrapper() { hearth_wrapper "$@"; }
harness_program() { hearth_program "$@"; }
harness_appimage() { hearth_appimage "$@"; }
harness_unpack() { hearth_unpack "$@"; }
harness_file_version() { hearth_file_version "$@"; }
