#!/usr/bin/env bash
# Install what the Pico 6502 firmware and examples need besides Rust:
# picotool (to make the .uf2 and to flash), the 64tass assembler, and CMake
# and the Pico SDK that picotool is built with.  Also adds the Rust target.
#
#   tools/setup-toolchain.sh [DIR]      DIR defaults to ../6502-Pico-Build
#
# Everything is installed inside DIR, together with DIR/env.sh, which sets up
# PATH.  Nothing is installed system-wide.  Re-running skips whatever is
# already there.  The folder layout and versions are those of the C
# Pico2-6502 project, so the two projects can share one DIR.
#
# picotool can flash and reboot the Pico only when built with libusb; on
# Debian or Ubuntu, install libusb-1.0-0-dev first.  Re-running this script
# after installing it rebuilds a picotool that lacks USB support.
set -euo pipefail

PROJECT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
DIR="${1:-$PROJECT/../6502-Pico-Build}"
mkdir -p "$DIR"
DIR="$(cd "$DIR" && pwd)"
cd "$DIR"

CMAKE_VERSION=3.31.6
PICO_SDK_VERSION=2.1.1          # picotool must use the same version
TASS_VERSION=1.60.3243
JOBS="$(nproc)"

step() { printf '\n== %s\n' "$*"; }

step "Rust target thumbv8m.main-none-eabihf"
rustup target add thumbv8m.main-none-eabihf

if [ ! -x cmake/bin/cmake ]; then
    step "CMake $CMAKE_VERSION"
    name="cmake-$CMAKE_VERSION-linux-x86_64"
    curl -fL -o cmake.tar.gz "https://github.com/Kitware/CMake/releases/download/v$CMAKE_VERSION/$name.tar.gz"
    rm -rf cmake "$name"
    tar xzf cmake.tar.gz
    mv "$name" cmake
    rm cmake.tar.gz
fi

export PATH="$DIR/cmake/bin:$PATH"

if [ ! -f pico-sdk/pico_sdk_init.cmake ]; then
    step "Pico SDK $PICO_SDK_VERSION"
    rm -rf pico-sdk
    git clone -b "$PICO_SDK_VERSION" --depth 1 https://github.com/raspberrypi/pico-sdk.git
    git -C pico-sdk submodule update --init --depth 1 lib/tinyusb
fi

has_libusb() { pkg-config --exists libusb-1.0 2>/dev/null; }
installed_picotool() { find picotool-install -name picotool -type f -perm -u+x 2>/dev/null | head -n 1 || true; }
picotool_has_usb() { "$1" help 2>/dev/null | grep -q '^ *load '; }

tool="$(installed_picotool)"
if [ -z "$tool" ] || { has_libusb && ! picotool_has_usb "$tool"; }; then
    step "picotool $PICO_SDK_VERSION"
    if ! has_libusb; then
        echo "note: libusb-1.0 not found; picotool will only make .uf2 files," >&2
        echo "      not flash. Install libusb-1.0-0-dev and re-run to add USB." >&2
    fi
    if [ ! -f picotool/CMakeLists.txt ]; then
        rm -rf picotool
        git clone -b "$PICO_SDK_VERSION" --depth 1 https://github.com/raspberrypi/picotool.git
    fi
    cmake -S picotool -B picotool/build -DCMAKE_BUILD_TYPE=Release \
        -DPICO_SDK_PATH="$DIR/pico-sdk" \
        -DCMAKE_INSTALL_PREFIX="$DIR/picotool-install" -DPICOTOOL_FLAT_INSTALL=1 \
        -U 'LIBUSB*' -U 'PC_LIBUSB*'
    cmake --build picotool/build -j"$JOBS"
    cmake --install picotool/build
fi

if [ ! -x bin/64tass ]; then
    step "64tass $TASS_VERSION"
    rm -rf "64tass-$TASS_VERSION-src"
    curl -fL -o 64tass.zip "https://sourceforge.net/projects/tass64/files/source/64tass-$TASS_VERSION-src.zip/download"
    unzip -q 64tass.zip
    rm 64tass.zip
    make -C "64tass-$TASS_VERSION-src" -j"$JOBS"
    mkdir -p bin
    cp "64tass-$TASS_VERSION-src/64tass" bin/
fi

step "Writing $DIR/env.sh"
cat > env.sh <<'ENV'
# Build environment for the Pico 6502 firmware (written by setup-toolchain.sh).
# Load it into a shell with:  . /path/to/6502-Pico-Build/env.sh
BUILD_ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
export PICO_SDK_PATH="$BUILD_ROOT/pico-sdk"
export picotool_DIR="$(dirname "$(find "$BUILD_ROOT/picotool-install" -name picotoolConfig.cmake -print -quit)")"
export PATH="$BUILD_ROOT/arm-gnu-toolchain/bin:$BUILD_ROOT/cmake/bin:$BUILD_ROOT/bin:$picotool_DIR:$PATH"
ENV

step "Done"
echo "Build the firmware with: $PROJECT/tools/build-firmware.sh $DIR"
