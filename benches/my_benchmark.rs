#![allow(unused_imports)] // Do not change this, (or the next) line.
#![allow(dead_code)]


use std::hint::black_box;
use criterion::{criterion_group, criterion_main, Criterion};
use std::collections::HashMap;
use std::collections::VecDeque;
use minimonitor::{DiskMeasurement, read_diskspaces};

fn bench_read_dms() {
    let mut hm = HashMap::<String, VecDeque<DiskMeasurement>>::new();
    let now = 0;
    read_diskspaces(now, &mut hm);

}

fn criterion_benchmark(c: &mut Criterion) {
    c.bench_function("read disk measurements", |b| b.iter(|| bench_read_dms()));
}

criterion_group!(benches, criterion_benchmark);
criterion_main!(benches);
