#!/bin/bash

version=$(grep "^version" <Cargo.toml | cut -d '"' -f 2)
echo "Building version $version"

cargo build --release

cd target/release || exit 1

tar -czf "mprisence-$version-x86_64.tar.gz" mprisence

sha256sum "mprisence-$version-x86_64.tar.gz"

cd ../..
