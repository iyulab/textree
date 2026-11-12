#!/bin/sh
# Builds the native library for Linux (glibc) in a container, without a cross toolchain.
#   dotnet/scripts/build-linux-native.sh amd64|arm64
# Output: target/linux-<arch>/libtextree_ffi.so
# Then build or publish the .NET projects with -p:TextreeNativeLibrary=<that file>.
set -eu
arch="${1:?usage: build-linux-native.sh amd64|arm64}"
root="$(cd "$(dirname "$0")/../.." && pwd)"
out="target/linux-$arch"
docker run --rm --platform "linux/$arch" \
  -v "$root:/src" -w /src \
  -e CARGO_TARGET_DIR="/src/$out/build" \
  rust:1-bookworm \
  cargo build --release --quiet -p textree-ffi
cp "$root/$out/build/release/libtextree_ffi.so" "$root/$out/libtextree_ffi.so"
echo "$root/$out/libtextree_ffi.so"
