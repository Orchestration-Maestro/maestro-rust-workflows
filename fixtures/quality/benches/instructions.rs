//! Instruction-count baseline and regression proof.
use gungraun::{library_benchmark, library_benchmark_group, main};
use std::hint::black_box;

#[library_benchmark]
fn doubled() -> u64 {
    quality::double(black_box(21))
}

library_benchmark_group!(name = arithmetic; benchmarks = doubled);
main!(library_benchmark_groups = arithmetic);
