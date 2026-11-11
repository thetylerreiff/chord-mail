#!/usr/bin/env bash
# Installs Chord, keyboard-driven Gmail for Omarchy.
#
#   curl -fsSL https://chord.thetylerreiff.co/install | bash
#
# It downloads the latest release package, checks it against its checksum,
# installs it with pacman, and enables the background service. Release
# packages carry Chord's Google sign-in. Where there's no package (aarch64, or
# before the first release) it builds from source instead, and a source build
# signs in with your own Google client. Run it again to update.
#
# Settings, all optional:
#   CHORD_REF=v0.1.0   install that release; a branch name builds it from source
#   CHORD_SOURCE=1     build from source even where a package exists
#   CHORD_SRC=<dir>    keep the source somewhere else

set -euo pipefail

# Everything runs from main, called on the last line, so a download that
# stops halfway runs nothing at all.
main() {
  local repo=https://github.com/thetylerreiff/chord-mail
  local ref=${CHORD_REF:-}
  local src=${CHORD_SRC:-${XDG_CACHE_HOME:-$HOME/.cache}/chord-mail/src}

  if [[ -t 1 ]]; then
    bold=$'\e[1m' dim=$'\e[2m' signal=$'\e[38;2;255;91;31m' reset=$'\e[0m'
  else
    bold='' dim='' signal='' reset=''
  fi

  ((EUID != 0)) || die "Run this as your own user, not root. It asks for sudo when it needs it."
  command -v pacman >/dev/null || die "Chord installs as an Arch package, so it needs Arch Linux or Omarchy."
  case $(uname -m) in
    x86_64 | aarch64) ;;
    *) die "Chord builds for x86_64 and aarch64, not $(uname -m)." ;;
  esac

  printf '\n  %sCHORD%s %s/ keyboard-driven Gmail for Omarchy%s\n\n' "$bold" "$reset" "$dim" "$reset"

  # A release package, unless this machine or the settings call for source.
  local built_from=source
  if [[ -z ${CHORD_SOURCE:-} && $(uname -m) == x86_64 && ( -z $ref || $ref == v[0-9]* ) ]]; then
    if install_release "$repo" "$ref"; then
      built_from=release
    elif [[ -n $ref ]]; then
      die "Couldn't download release $ref. Check the version at $repo/releases."
    else
      printf '  No release package yet, so Chord builds from source instead.\n'
    fi
  fi
  [[ $built_from == release ]] || build_from_source "$repo" "${ref:-main}" "$src"

  step "" "Background service"
  # It waits quietly until you sign in, then syncs, wakes snoozes, sends
  # scheduled mail, and notifies for new Primary mail.
  if systemctl --user enable --now chord-mail.service 2>/dev/null; then
    echo "chord-mail.service is running."
    # An update leaves the old binary running until the service restarts.
    systemctl --user try-restart chord-mail.service 2>/dev/null || true
  else
    echo "No user session here. Later, run: systemctl --user enable --now chord-mail"
  fi

  printf '\n  %sDone.%s Open Chord from the launcher (Super+Space, "Chord")' "$bold" "$reset"
  if [[ $built_from == release ]]; then
    printf ' and press Enter\n  to sign in with Google.\n\n'
  else
    printf '. A source build has no\n  built-in Google sign-in, so connect it with your own Google client:\n'
    printf '  %s%s#use-your-own-google-client%s\n\n' "$dim" "$repo" "$reset"
  fi
  printf '  To open mailto: links in Chord:  %schord-mail default%s\n\n' "$bold" "$reset"
}

# Downloads a release package (the latest, or the tag given), checks it
# against its published checksum, and installs it. Fails without changing
# anything if there's no such package.
install_release() {
  local repo=$1 ref=$2 name=chord-mail-x86_64.pkg.tar.zst url
  if [[ -n $ref ]]; then url=$repo/releases/download/$ref; else url=$repo/releases/latest/download; fi
  tmp=$(mktemp -d)
  trap 'rm -rf "$tmp"' EXIT
  step "" "Package (${ref:-latest release})"
  # Only a 404 means there's no such release; anything else is a real failure.
  local code
  code=$(curl -sSL -o "$tmp/$name" -w '%{http_code}' "$url/$name") || code=000
  [[ $code == 404 ]] && return 1
  [[ $code == 200 ]] || die "Couldn't download the package (HTTP $code), so nothing was installed. Try again."
  code=$(curl -sSL -o "$tmp/$name.sha256" -w '%{http_code}' "$url/$name.sha256") || code=000
  [[ $code == 200 ]] || die "Couldn't download the package checksum (HTTP $code), so nothing was installed. Try again."
  (cd "$tmp" && sha256sum --check --quiet "$name.sha256") \
    || die "The download didn't match its checksum, so nothing was installed. Try again."
  sudo pacman -U --noconfirm "$tmp/$name" \
    || die "pacman couldn't install the package. A likely cause is an out-of-date system: run sudo pacman -Syu, then try again."
}

build_from_source() {
  local repo=$1 ref=$2 src=$3
  step "" "Build tools"
  local tools=(base-devel git lld)
  if ! cargo --version &>/dev/null; then
    if command -v rustup >/dev/null; then
      rustup default stable
    else
      tools+=(rust)
    fi
  fi
  sudo pacman -S --needed --noconfirm "${tools[@]}"

  step "" "Source ($ref)"
  if [[ -d $src/.git ]]; then
    git -C "$src" fetch --depth 1 origin "$ref"
    git -C "$src" checkout --quiet --force FETCH_HEAD
  else
    rm -rf "$src"
    mkdir -p "$(dirname "$src")"
    git clone --depth 1 --branch "$ref" "$repo.git" "$src"
  fi

  # The test suite already ran in CI for this commit.
  step "" "Build and install the package"
  "$src/bin/install" --nocheck
}

# Numbered steps; the count depends on whether Chord came as a package.
step_n=0
step() {
  step_n=$((step_n + 1))
  printf '\n  %s%02d%s  %s%s%s\n' "$signal" "$step_n" "$reset" "$bold" "$2" "$reset"
}

die() {
  printf '\n  chord: %s\n\n' "$1" >&2
  exit 1
}

main "$@"
