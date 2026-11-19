#!/bin/sh
# Host-only isolated NuraBrowser Wayland test. No package installation.
set -eu
exec python3 "$(dirname "$0")/qualify-browser-nested-wayland.py" "$@"
