#!/usr/bin/env bash

die() {
  printf '%s\n' "$*" >&2
  exit 1
}

require_command() {
  command -v "$1" >/dev/null 2>&1 || die "missing required command: $1"
}

replace_directory() {
  local stage="$1"
  local destination="$2"
  local previous="${destination}.previous"

  case "$destination" in
    "$MAV_ROOT"/apps/ios/build/* | "$MAV_ROOT"/apps/android/build/*) ;;
    *) die "refusing to replace unexpected path: $destination" ;;
  esac

  rm -rf "$previous"
  if [[ -e "$destination" ]]; then
    mv "$destination" "$previous"
  fi
  if mv "$stage" "$destination"; then
    rm -rf "$previous"
    return
  fi
  if [[ -e "$previous" ]]; then
    mv "$previous" "$destination"
  fi
  die "could not replace package directory: $destination"
}

write_checksums() {
  local directory="$1"
  local -a checksum_command

  if command -v sha256sum >/dev/null 2>&1; then
    checksum_command=(sha256sum)
  elif command -v shasum >/dev/null 2>&1; then
    checksum_command=(shasum -a 256)
  else
    die "missing SHA-256 command"
  fi

  (
    cd "$directory"
    find . -type f ! -name SHA256SUMS -print |
      LC_ALL=C sort |
      while IFS= read -r file; do
        "${checksum_command[@]}" "$file"
      done >SHA256SUMS
  )
}
