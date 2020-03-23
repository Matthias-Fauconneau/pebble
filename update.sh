#!/bin/sh
cargo update --manifest-path kernel/Cargo.toml
cargo update --manifest-path lib/libpebble/Cargo.toml
cargo update --manifest-path lib/mer/Cargo.toml
cargo update --manifest-path lib/pebble_util/Cargo.toml
