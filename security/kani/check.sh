#!/usr/bin/env bash
set -euo pipefail

if [[ "$(uname -s)-$(uname -m)" != "Linux-x86_64" ]]; then
    printf '%s\n' 'The pinned Kani bundle requires Linux x86-64.' >&2
    exit 1
fi

repo_dir=$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")/../.." && pwd)
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

# The standalone driver can leave build files next to its input on failure.
mkdir "$work_dir/src"
(
    cd "$repo_dir"
    cp --parents -- \
        security/kani/ticket_redemption.rs \
        security/kani/ssh_session_deadline.rs \
        warpgate-common/src/ticket_policy.rs \
        warpgate-protocol-ssh/src/server/session_deadline.rs \
        "$work_dir/src"
)
cd "$work_dir/src"
status=0
for harness in ticket_redemption ssh_session_deadline; do
    printf '\nVerifying %s\n' "$harness"
    if ! RUSTUP_TOOLCHAIN="$toolchain" "$bundle/bin/kani-driver" "security/kani/$harness.rs" \
        --output-format terse -Z concrete-playback --concrete-playback print; then
        status=1
    fi
done
exit "$status"
