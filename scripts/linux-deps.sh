#!/usr/bin/env bash
# System libraries needed to build the DAW on Debian/Ubuntu.
# Usage: sudo bash scripts/linux-deps.sh [--all]
#   (default) ALSA + GUI libraries — enough for the standard build
#   --all     also JACK + PipeWire dev files and clang (for --features jack,pipewire)
set -euo pipefail

BASE=(
  pkg-config
  libasound2-dev        # ALSA (cpal's default Linux backend)
  libxkbcommon-dev      # keyboard handling (winit)
  libwayland-dev        # Wayland windows
  libx11-dev libxcursor-dev libxrandr-dev libxi-dev   # X11 windows
  libgl1-mesa-dev       # OpenGL (egui glow renderer)
)
EXTRA=(
  libjack-jackd2-dev    # --features jack
  libpipewire-0.3-dev   # --features pipewire
  clang libclang-dev    # bindgen, needed by the PipeWire bindings
)

PKGS=("${BASE[@]}")
if [[ "${1:-}" == "--all" || -n "${CI:-}" ]]; then
  PKGS+=("${EXTRA[@]}")
fi

export DEBIAN_FRONTEND=noninteractive
apt-get update -qq
apt-get install -y -qq "${PKGS[@]}"
echo "Installed: ${PKGS[*]}"
