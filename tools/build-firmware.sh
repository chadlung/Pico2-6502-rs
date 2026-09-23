#!/usr/bin/env bash
# Build the Pico 6502 firmware and its UF2 image.
#
#   tools/build-firmware.sh [DIR]      DIR defaults to ../6502-Pico-Build
#
# The firmware is written to target/thumbv8m.main-none-eabihf/release/: the
# ELF pico6502, and pico6502.uf2, which needs picotool from DIR (installed by
# tools/setup-toolchain.sh) or from PATH.
set -euo pipefail

PROJECT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
DIR="${1:-$PROJECT/../6502-Pico-Build}"
if [ -f "$DIR/env.sh" ]; then
    . "$DIR/env.sh"
fi

cd "$PROJECT"
cargo build --release --no-default-features --features firmware \
    --bin pico6502 --target thumbv8m.main-none-eabihf

ELF="$PROJECT/target/thumbv8m.main-none-eabihf/release/pico6502"
echo
if command -v picotool >/dev/null; then
    picotool uf2 convert -t elf "$ELF" "$ELF.uf2"
    echo "Firmware: $ELF.uf2"
else
    echo "Firmware: $ELF"
    echo "picotool not found, so no .uf2; run tools/setup-toolchain.sh" >&2
fi
