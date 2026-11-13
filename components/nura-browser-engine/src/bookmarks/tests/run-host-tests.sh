#!/bin/sh
# SPDX-License-Identifier: MIT
# Host-only private bookmark store: no WebKit, compositor or device.
set -eu
here=$(CDPATH='' cd -- "$(dirname -- "$0")" && pwd)
root=$(CDPATH='' cd -- "$here/../../.." && pwd)
tmp=$(mktemp -d)
trap 'rm -rf "$tmp"' EXIT HUP INT TERM
cc -std=c11 -D_POSIX_C_SOURCE=200809L -O1 -g -Wall -Wextra -Werror -Wpedantic \
    -I"$root/src/bookmarks" -I"$root/src/navigation" \
    $(pkg-config --cflags glib-2.0 gio-2.0 json-glib-1.0) \
    "$root/src/bookmarks/bookmarks.c" \
    "$root/src/navigation/state.c" \
    "$root/src/navigation/url.c" \
    "$here/test_bookmarks.c" \
    $(pkg-config --libs glib-2.0 gio-2.0 json-glib-1.0) \
    -o "$tmp/test-bookmarks"
G_DEBUG=fatal-warnings "$tmp/test-bookmarks" "$@"
