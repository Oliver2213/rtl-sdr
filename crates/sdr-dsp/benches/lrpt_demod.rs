//! LRPT stage-1 demod throughput bench (epic #469 + issue #662).
//!
//! Measures the end-to-end demod chain on 1 second of synthetic
//! 144 ksps input — both the QPSK pipeline (epic #469 baseline)
//! and the OQPSK pipeline (#662, dbdexter port). Establishes the
//! perf floor for regression detection on either path.

use criterion::{Criterion, black_box, criterion_group, criterion_main};
use sdr_dsp::lrpt::{LrptDemod, LrptMode};
use sdr_types::Complex;

/// 1 second of input at the demod's 144 ksps working sample rate.
const SAMPLES_1S: usize = 144_000;

fn bench_demod_qpsk(c: &mut Criterion) {
    let symbols = [
        Complex::new(0.707, 0.707),
        Complex::new(-0.707, 0.707),
        Complex::new(0.707, -0.707),
        Complex::new(-0.707, -0.707),
    ];
    let buf: Vec<Complex> = (0..SAMPLES_1S)
        .map(|n| {
            if n % 2 == 0 {
                symbols[(n / 2) % 4]
            } else {
                Complex::new(0.0, 0.0)
            }
        })
        .collect();

    c.bench_function("lrpt_demod_qpsk_1s_144ksps", |b| {
        b.iter(|| {
            let mut demod = LrptDemod::new().expect("LrptDemod::new");
            let mut emitted = 0_u32;
            for s in &buf {
                if demod.process(black_box(*s)).is_some() {
                    emitted += 1;
                }
            }
            black_box(emitted);
        });
    });
}

fn bench_demod_oqpsk(c: &mut Criterion) {
    // OQPSK input: I-only sample on even indices, Q-only on odd
    // (the canonical "Q delayed by Tsym/2" representation at
    // 2 sps).
    let i_vals = [0.707_f32, -0.707, 0.707, -0.707];
    let q_vals = [0.707_f32, 0.707, -0.707, -0.707];
    let buf: Vec<Complex> = (0..SAMPLES_1S)
        .map(|n| {
            let sym_idx = (n / 2) % 4;
            if n % 2 == 0 {
                Complex::new(i_vals[sym_idx], 0.0)
            } else {
                Complex::new(0.0, q_vals[sym_idx])
            }
        })
        .collect();

    c.bench_function("lrpt_demod_oqpsk_1s_144ksps", |b| {
        b.iter(|| {
            let mut demod =
                LrptDemod::new_with_mode(LrptMode::Oqpsk).expect("LrptDemod::new_with_mode");
            let mut emitted = 0_u32;
            for s in &buf {
                if demod.process(black_box(*s)).is_some() {
                    emitted += 1;
                }
            }
            black_box(emitted);
        });
    });
}

criterion_group!(benches, bench_demod_qpsk, bench_demod_oqpsk);
criterion_main!(benches);
