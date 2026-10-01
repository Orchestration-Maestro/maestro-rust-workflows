//! Hosted native cache transport: real consumer preparation and coverage execution evidence.

#![cfg(test)]
#![forbid(unsafe_code)]

mod harness;

use crate::harness::{prepare_hosted_fixture, verify_hosted_fixture};

#[test]
fn prepare_the_committed_native_consumer_fixture() {
    prepare_hosted_fixture();
}

#[test]
fn verify_the_executed_native_coverage_fixture() {
    verify_hosted_fixture();
}
