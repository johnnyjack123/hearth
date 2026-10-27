# Helpers for app recipes (crates/harness-core/defaults/apps/*.toml).
#
# harness runs this file and the recipe's `install` as one script with `sh`
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
harness_download() {
  curl -fsSL --retry 3 --proto '=https' --proto-redir '=https' -o "$2" "$1"
}

# Fails unless file $1 has the SHA-256 checksum $2.
harness_sha256() {
  echo "$2  $1" | sha256sum -c --quiet -
}

# Records the installed version for `harness app list` and the browser.
# Called as harness_version "$(program --version)": a program that is
# missing or fails only leaves the argument empty, so that fails here.
harness_version() {
  if [ -z "${1:-}" ]; then
    echo "Version nicht ermittelbar: Das installierte Programm startet nicht." >&2
    exit 1
  fi
  printf '%s\n' "$1" > "$HARNESS_VERSION_FILE"
}

# apt without root rights and without touching the system: lists and
# downloads stay in $WORK. The image's apt.conf.d is left out: Debian's
# images clean /var/cache/apt there after every update, which fails without
# root. apt reads that folder before any -o option, so only APT_CONFIG can
# point it elsewhere; the folder's two other settings come back as options.
_harness_apt_get() {
  APT_CONFIG="$WORK/apt/apt.conf" apt-get -q -o "Dir::State::Lists=$WORK/apt/lists" \
    -o "Dir::Cache=$WORK/apt/cache" -o Debug::NoLocking=1 -o "APT::Sandbox::User=$(id -un)" \
    -o Acquire::Languages=none -o Acquire::GzipIndexes=true "$@"
}

# Unpacks Debian packages, with the dependencies the base image lacks, into
# $PREFIX/root. Packages the base image has stay out even if the archive has
# a newer version: two versions of one library side by side would be worse
# than headers of a newer point release.
harness_apt() {
  mkdir -p "$WORK/apt/conf.d" "$WORK/apt/lists/partial" "$WORK/apt/cache/archives/partial" \
    "$WORK/apt/debs" "$PREFIX/root"
  echo "Dir::Etc::Parts \"$WORK/apt/conf.d\";" > "$WORK/apt/apt.conf"
  _harness_apt_get update -qq
  # "Inst name (version …)" is a new package, "Inst name [old] (new …)" an
  # upgrade of one the base image has.
  packages=$(_harness_apt_get install -s --no-install-recommends "$@" \
    | sed -n 's/^Inst \([^ ]*\) (.*/\1/p')
  if [ -z "$packages" ]; then
    echo "Schon im Base-Image: $*"
    return 0
  fi
  echo "Pakete: $(echo $packages)"
  # shellcheck disable=SC2086 # one word per package
  (cd "$WORK/apt/debs" && _harness_apt_get download $packages)
  for deb in "$WORK/apt/debs"/*.deb; do
    dpkg-deb -x "$deb" "$PREFIX/root"
  done
  rm -rf "$WORK/apt/debs"
}

# Library folders of the packages harness_apt unpacked.
harness_libs() {
  echo "$PREFIX/root/usr/lib/$MACHINE-linux-gnu:$PREFIX/root/usr/lib"
}

# Writes a wrapper to $PREFIX/bin for every program in the given folders
# below $PREFIX/root (default usr/bin). Each finds the unpacked libraries
# through LD_LIBRARY_PATH, set only for it and what it starts.
harness_wrap() {
  [ $# -gt 0 ] || set -- usr/bin
  mkdir -p "$PREFIX/bin"
  for dir in "$@"; do
    for program in "$PREFIX/root/$dir"/*; do
      [ -f "$program" ] && [ -x "$program" ] || continue
      harness_wrapper "${program##*/}" "$program"
    done
  done
}

# Writes the wrapper $PREFIX/bin/$1 for program $2; further arguments go
# before the caller's.
harness_wrapper() {
  name=$1
  program=$2
  shift 2
  {
    echo '#!/bin/sh'
    printf 'LD_LIBRARY_PATH=%s${LD_LIBRARY_PATH:+:$LD_LIBRARY_PATH} exec %s' \
      "$(harness_libs)" "$program"
    for argument in "$@"; do
      printf ' %s' "$argument"
    done
    echo ' "$@"'
  } > "$PREFIX/bin/$name"
  chmod 755 "$PREFIX/bin/$name"
}
