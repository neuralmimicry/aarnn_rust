#!/usr/bin/env bash
set -euo pipefail

die() {
  printf 'error: %s\n' "$*" >&2
  exit 1
}

if (($# != 2)); then
  die "usage: $0 <downloaded-artifacts-dir> <empty-staging-dir>"
fi

source_dir="$1"
staging_dir="$2"
[[ -d "$source_dir" ]] || die "artifact directory does not exist: $source_dir"
mkdir -p -- "$staging_dir"
source_dir="$(cd -- "$source_dir" && pwd)"
staging_dir="$(cd -- "$staging_dir" && pwd)"

case "$staging_dir" in
  "$source_dir"|"$source_dir"/*)
    die "staging directory must be outside the downloaded artifact directory"
    ;;
esac

if [[ -n "$(find "$staging_dir" -mindepth 1 -print -quit)" ]]; then
  die "staging directory must be empty: $staging_dir"
fi

mapfile -d '' sources < <(
  find "$source_dir" -type f \
    \( -name '*.deb' -o -name '*.tar.gz' -o -name '*.sha256.txt' \) \
    -print0 | sort -z
)
((${#sources[@]} > 0)) || die "no release assets found under $source_dir"

for source in "${sources[@]}"; do
  source_name="$(basename -- "$source")"
  case "$source_name" in
    *.deb)
      # GitHub Releases publishes '~' in asset names as '.'. Stage the file
      # under its final public name so its checksum manifest can be verified.
      release_name="${source_name//\~/.}"
      [[ ! -e "$staging_dir/$release_name" ]] || die "multiple artifacts normalize to $release_name"
      cp -- "$source" "$staging_dir/$release_name"
      ;;
    *.sha256.txt)
      release_name="$source_name"
      [[ ! -e "$staging_dir/$release_name" ]] || die "duplicate release asset: $release_name"
      # Match the Debian asset-name normalization applied by the release API.
      sed 's/~/./g' "$source" >"$staging_dir/$release_name"
      ;;
    *.tar.gz)
      release_name="$source_name"
      [[ ! -e "$staging_dir/$release_name" ]] || die "duplicate release asset: $release_name"
      cp -- "$source" "$staging_dir/$release_name"
      ;;
  esac

  [[ -f "$staging_dir/$release_name" ]] || die "failed to stage $source_name"
done

shopt -s nullglob
manifests=("$staging_dir"/*.sha256.txt)
packages=("$staging_dir"/*.deb)
((${#manifests[@]} > 0)) || die "no checksum manifests were staged"
((${#packages[@]} > 0)) || die "no Debian packages were staged"

for package in "${packages[@]}"; do
  package_name="$(basename -- "$package")"
  if ! grep -Fq -- "  $package_name" "${manifests[@]}"; then
    die "no checksum manifest references $package_name"
  fi
done

(
  cd -- "$staging_dir"
  sha256sum -c "${manifests[@]##*/}"
)
