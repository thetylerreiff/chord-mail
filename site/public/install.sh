#!/usr/bin/env bash
# Installs Chord, keyboard-driven Gmail for Omarchy.
#
#   curl -fsSL https://chord.thetylerreiff.co/install | bash
#
# It installs the build tools with pacman, fetches the source into
# ~/.cache/chord-mail/src, builds the Arch package with makepkg, installs it,
# and enables the background service. Run it again to update.
#
# Settings, all optional:
#   CHORD_REF=v0.1.0   build a tag or branch instead of main
#   CHORD_SRC=<dir>    keep the source somewhere else

set -euo pipefail

# Everything runs from main, called on the last line, so a download that
# stops halfway runs nothing at all.
main() {
  local repo=https://github.com/thetylerreiff/chord-mail.git
  local ref=${CHORD_REF:-main}
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

  step 01 "Build tools"
  local tools=(base-devel git lld)
  if ! cargo --version &>/dev/null; then
    if command -v rustup >/dev/null; then
      rustup default stable
    else
      tools+=(rust)
    fi
  fi
  sudo pacman -S --needed --noconfirm "${tools[@]}"

  step 02 "Source ($ref)"
  if [[ -d $src/.git ]]; then
    git -C "$src" fetch --depth 1 origin "$ref"
    git -C "$src" checkout --quiet --force FETCH_HEAD
  else
    rm -rf "$src"
    mkdir -p "$(dirname "$src")"
    git clone --depth 1 --branch "$ref" "$repo" "$src"
  fi

  # The test suite already ran in CI for this commit.
  step 03 "Build and install the package"
  "$src/bin/install" --nocheck

  step 04 "Background service"
  # It waits quietly until you sign in, then syncs, wakes snoozes, sends
  # scheduled mail, and notifies for new Primary mail.
  if systemctl --user enable --now chord-mail.service 2>/dev/null; then
    echo "chord-mail.service is running."
  else
    echo "No user session here. Later, run: systemctl --user enable --now chord-mail"
  fi

  printf '\n  %sDone.%s Open Chord from the launcher (Super+Space, "Chord") and it\n' "$bold" "$reset"
  printf '  walks you through connecting Gmail:\n'
  printf '  %shttps://github.com/thetylerreiff/chord-mail#connect-gmail%s\n\n' "$dim" "$reset"
  printf '  To open mailto: links in Chord:  %schord-mail default%s\n\n' "$bold" "$reset"
}

step() {
  printf '\n  %s%s%s  %s%s%s\n' "$signal" "$1" "$reset" "$bold" "$2" "$reset"
}

die() {
  printf '\n  chord: %s\n\n' "$1" >&2
  exit 1
}

main "$@"
