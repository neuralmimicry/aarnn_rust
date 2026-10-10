#!/usr/bin/env bash
set -euo pipefail

script_dir="$(CDPATH='' cd -- "$(dirname -- "${BASH_SOURCE[0]}")" && pwd)"
test_root="$(mktemp -d "${TMPDIR:-/tmp}/aarnn-release-assets-test.XXXXXX")"
source_dir="$test_root/input"
staging_dir="$test_root/staged"
mkdir -p "$source_dir" "$staging_dir"

deb_name='aarnn-rust_0.1.39~dev.344.1696e0f_ubuntu24.04_amd64.deb'
archive_name='aarnn_rust-0.1.39-dev.344.1696e0f-linux-x86_64-ubuntu24.04.tar.gz'
manifest_name='aarnn_rust-0.1.39-dev.344.1696e0f-linux-x86_64-ubuntu24.04.sha256.txt'
printf 'fixture Debian package bytes\n' >"$source_dir/$deb_name"
printf 'fixture archive bytes\n' >"$source_dir/$archive_name"
(
  cd -- "$source_dir"
  sha256sum "$archive_name" "$deb_name" >"$manifest_name"
)

bash "$script_dir/prepare-release-assets.sh" "$source_dir" "$staging_dir"

normalized_deb_name="${deb_name//\~/.}"
[[ -f "$staging_dir/$normalized_deb_name" ]]
grep -Fq -- "  $normalized_deb_name" "$staging_dir/$manifest_name"
(
  cd -- "$staging_dir"
  sha256sum -c "$manifest_name"
)

printf 'release asset normalization regression test passed\n'
