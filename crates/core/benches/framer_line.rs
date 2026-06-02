//! Criterion benchmarks for the line framer.

#![allow(missing_docs)]

use bytes::BytesMut;
use criterion::{black_box, criterion_group, criterion_main, BatchSize, Criterion};
use tracemux_core::framer::line::{Eol, LineFramer};
use tracemux_core::framer::Framer;

fn line_input(lines: usize, line_len: usize, eol: &[u8]) -> Vec<u8> {
    let mut input = Vec::with_capacity(lines * (line_len + eol.len()));
    for idx in 0..lines {
        let line = format!(
            "line-{idx:04}-{:0>width$}",
            "",
            width = line_len.saturating_sub(10)
        );
        input.extend_from_slice(line.as_bytes());
        input.extend_from_slice(eol);
    }
    input
}

fn drain_lines(eol: Eol, input: &[u8]) -> usize {
    let mut framer = LineFramer::new(eol, input.len() + 1);
    let mut buf = BytesMut::from(input);
    let mut frames = 0usize;

    while let Some(frame) = framer.poll_frame(&mut buf).expect("line frame") {
        black_box(frame);
        frames += 1;
    }

    frames
}

fn bench_line_framer(c: &mut Criterion) {
    let lf_input = line_input(4096, 64, b"\n");
    let mixed_input = {
        let mut input = Vec::new();
        input.extend_from_slice(&line_input(2048, 64, b"\r\n"));
        input.extend_from_slice(&line_input(2048, 64, b"\n"));
        input
    };

    c.bench_function("line_framer_lf_4096x64", |b| {
        b.iter_batched(
            || lf_input.clone(),
            |input| black_box(drain_lines(Eol::Lf, &input)),
            BatchSize::LargeInput,
        );
    });

    c.bench_function("line_framer_auto_mixed_4096x64", |b| {
        b.iter_batched(
            || mixed_input.clone(),
            |input| black_box(drain_lines(Eol::Auto, &input)),
            BatchSize::LargeInput,
        );
    });
}

criterion_group!(benches, bench_line_framer);
criterion_main!(benches);
