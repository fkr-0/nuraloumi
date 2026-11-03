// Standalone compilation until provider-lane owns pub mod maps in lib.rs.
// The harness imports shared provider modules without their normal consumers.
#![allow(dead_code)]
// rustc --edition 2021 --test tests/fixtures/maps/module_harness.rs -o target/nuraloumi-maps-tests
#[path = "../../../crates/nuraloumi-providers/src/error.rs"]
mod error;
#[path = "../../../crates/nuraloumi-providers/src/command.rs"]
mod command;
#[path = "../../../crates/nuraloumi-providers/src/maps.rs"]
mod maps;
