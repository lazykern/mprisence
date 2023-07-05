#!/bin/bash

version=$(grep "^version" <Cargo.toml | cut -d '"' -f 2)
echo "Building version $version"

cargo build --release

tar -czf "target/release/mprisence-$version-x86_64.tar.gz" target/release/mprisence

sha256sum "target/release/mprisence-$version-x86_64.tar.gz"
