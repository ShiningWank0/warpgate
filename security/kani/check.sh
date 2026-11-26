#!/usr/bin/env bash
set -euo pipefail

if [[ "$(uname -s)-$(uname -m)" != "Linux-x86_64" ]]; then
    printf '%s\n' 'The pinned Kani bundle requires Linux x86-64.' >&2
    exit 1
fi

source_dir=$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")" && pwd)
work_dir=$(mktemp -d)
trap 'rm -rf -- "$work_dir"' EXIT
version=kani-0.68.0
archive="$work_dir/kani.tar.gz"
checksum=32e2b484d73ede0bbf64a2cf0879c4259422497d8aec4ee67d448de9ae7843d3

curl --fail --silent --show-error --location --retry 3 --proto '=https' --proto-redir '=https' \
    "https://github.com/model-checking/kani/releases/download/$version/$version-x86_64-unknown-linux-gnu.tar.gz" \
    --output "$archive"
printf '%s  %s\n' "$checksum" "$archive" | sha256sum --check
tar --extract --gzip --file "$archive" --directory "$work_dir"

bundle="$work_dir/$version"
toolchain=$(cat "$bundle/rust-toolchain-version")
rustup toolchain install "$toolchain" --profile minimal
sysroot=$(rustup run "$toolchain" rustc --print sysroot)
export PATH="$bundle/bin:$PATH"
export LD_LIBRARY_PATH="$sysroot/lib${LD_LIBRARY_PATH:+:$LD_LIBRARY_PATH}"

cd "$work_dir"
RUSTUP_TOOLCHAIN="$toolchain" "$bundle/bin/kani-driver" "$source_dir/ticket_redemption.rs"
