//! Polyphase interpolating root-raised-cosine matched filter for
//! Meteor LRPT.
//!
//! Faithful transliteration of dbdexter `meteor_demod/dsp/filter.c`
//! (`filter_init_rrc`, `rrc_coeff`, `filter_fwd_sample`,
//! `filter_get`). The filter is both the RRC matched filter AND the
//! fractional-delay interpolator the Mueller-Muller timing loop
//! steers: a single prototype of `taps*factor` coefficients is
//! polyphase-decomposed into `factor` sub-filters, and
//! [`RrcFilter::get`] evaluates sub-filter `phase` against the
//! complex history to produce the interpolated, matched-filtered
//! sample at that fractional offset.
//!
//! Reference (read-only): `original/meteor_demod/dsp/filter.c`
//! and `original/meteor_demod/demod.h` (`RRC_ALPHA`, `RRC_ORDER`,
//! `INTERP_FACTOR`).

use core::f32::consts::PI;

use sdr_types::Complex;

/// RRC design order. `meteor_demod/demod.h:12` (`RRC_ORDER 32`).
pub const RRC_ORDER: usize = 32;

/// Polyphase interpolation factor. `meteor_demod/demod.h:13`
/// (`INTERP_FACTOR 5`).
pub const INTERP_FACTOR: usize = 5;

/// Per-phase sub-filter length: `order*2+1`. `filter.c:12`
/// (`taps = order*2+1`). 65 taps.
pub const NUM_TAPS: usize = RRC_ORDER * 2 + 1;

/// Prototype length: `taps*factor`. `filter.c:15,20`. 325 coeffs,
/// laid out as `factor` contiguous sub-filters of `NUM_TAPS` each.
pub const NUM_COEFFS: usize = NUM_TAPS * INTERP_FACTOR;

/// Symbol-rate rolloff factor β. `meteor_demod/demod.h:8`
/// (`RRC_ALPHA 0.6`).
pub const ROLLOFF: f32 = 0.6;

/// Polyphase RRC matched filter / fractional-delay interpolator.
/// Complex baseband in, complex out (one [`RrcFilter::get`] per
/// polyphase phase after each [`RrcFilter::push`]).
pub struct RrcFilter {
    /// `factor` sub-filters of [`NUM_TAPS`] coefficients each, in
    /// sub-filter-major order (`coeffs[subfilter*NUM_TAPS + tap]`),
    /// matching `filter.c:20` `coeffs[j*taps + i]`.
    coeffs: [f32; NUM_COEFFS],
    /// Circular history of the last [`NUM_TAPS`] input samples
    /// (`filter.c` `flt->mem`).
    mem: [Complex; NUM_TAPS],
    /// Write cursor into `mem` (`filter.c` `flt->idx`).
    idx: usize,
}

impl RrcFilter {
    /// Build the polyphase RRC at oversampling factor `osf`
    /// (= input `samplerate / symrate`). Transliterates
    /// `filter_init_rrc` (`filter.c:9-28`): the prototype is
    /// evaluated at `osf*factor` and decomposed into `factor`
    /// phases.
    #[must_use]
    #[allow(
        clippy::cast_precision_loss,
        reason = "INTERP_FACTOR (= 5) converts to f32 exactly"
    )]
    pub fn new(osf: f32) -> Self {
        let mut coeffs = [0.0_f32; NUM_COEFFS];
        // filter.c:18-22 — coeffs[j*taps + i] = rrc_coeff(i*factor + j,
        //                   taps*factor, osf*factor, alpha)
        for j in 0..INTERP_FACTOR {
            for i in 0..NUM_TAPS {
                coeffs[j * NUM_TAPS + i] = rrc_coeff(
                    i * INTERP_FACTOR + j,
                    NUM_COEFFS,
                    osf * INTERP_FACTOR as f32,
                    ROLLOFF,
                );
            }
        }
        Self {
            coeffs,
            mem: [Complex::new(0.0, 0.0); NUM_TAPS],
            idx: 0,
        }
    }

    /// Push one input sample into the circular history.
    /// Transliterates `filter_fwd_sample` (`filter.c:38-43`).
    pub fn push(&mut self, sample: Complex) {
        self.mem[self.idx] = sample;
        self.idx += 1;
        self.idx %= NUM_TAPS;
    }

    /// Evaluate polyphase sub-filter for `phase` (0..[`INTERP_FACTOR`])
    /// against the current history. Transliterates `filter_get`
    /// (`filter.c:45-65`), including the sub-filter phase reversal
    /// `(interp_factor - phase - 1)` and the two-chunk circular walk
    /// starting at `idx`.
    pub fn get(&self, phase: usize) -> Complex {
        let mut result = Complex::new(0.0, 0.0);
        // filter.c:52 — j = (interp_factor - phase - 1) * size
        let mut j = (INTERP_FACTOR - phase - 1) * NUM_TAPS;
        // filter.c:55-57 — chunk 1: current position to end
        for i in self.idx..NUM_TAPS {
            result += self.mem[i] * self.coeffs[j];
            j += 1;
        }
        // filter.c:60-62 — chunk 2: start to current position - 1
        for i in 0..self.idx {
            result += self.mem[i] * self.coeffs[j];
            j += 1;
        }
        result
    }
}

/// Variable-alpha RRC filter coefficient. Faithful transliteration
/// of `rrc_coeff` (`filter.c:70-94`); formula from
/// <https://www.michael-joost.de/rrcfilter.pdf>. Applies the
/// (mislabeled-"Hamming"-in-C, actually **Blackman**) window
/// 0.42 / −0.5 / 0.08 and the fixed `norm = 2/5` scalar.
///
/// `stage_no` is the **absolute prototype tap index** (0..`taps`),
/// `taps` the prototype length, `osf` the prototype oversampling
/// (`samplerate/symrate × factor`).
#[allow(
    clippy::cast_precision_loss,
    clippy::cast_possible_wrap,
    clippy::cast_possible_truncation,
    reason = "stage_no/taps are small filter indices (< 325); the f32/i32 \
              conversions are exact in this range"
)]
fn rrc_coeff(stage_no: usize, taps: usize, osf: f32, alpha: f32) -> f32 {
    // filter.c:73 — const float norm = 2.0/5.0;
    const NORM: f32 = 2.0 / 5.0;
    // filter.c:79 — order = (taps - 1)/2;
    let order = (taps - 1) / 2;

    // filter.c:81-84 — handle the 0/0 case (center tap).
    if order == stage_no {
        return NORM * (1.0 - alpha + 4.0 * alpha / PI);
    }

    // filter.c:86 — t = abs(order - stage_no)/osf;  (integer abs, then /osf)
    let t = (order as i32 - stage_no as i32).unsigned_abs() as f32 / osf;
    // filter.c:87 — coeff = sin(πt(1-α)) + 4αt·cos(πt(1+α));
    let mut coeff =
        (PI * t * (1.0 - alpha)).sin() + 4.0 * alpha * t * (PI * t * (1.0 + alpha)).cos();
    // filter.c:88 — interm = πt(1 - (4αt)^2);
    let interm = PI * t * (1.0 - (4.0 * alpha * t) * (4.0 * alpha * t));

    // filter.c:91 — Blackman window keyed on the absolute index stage_no.
    let taps_m1 = (taps - 1) as f32;
    coeff *= 0.42 - 0.5 * (2.0 * PI * stage_no as f32 / taps_m1).cos()
        + 0.08 * (4.0 * PI * stage_no as f32 / taps_m1).cos();

    // filter.c:93 — return coeff / interm * norm;
    coeff / interm * NORM
}

#[cfg(test)]
#[allow(
    clippy::cast_precision_loss,
    reason = "test code converts small filter-size constants to f32; exact in range"
)]
mod tests {
    use super::*;

    /// The center prototype tap (`stage_no` == order) must equal
    /// `norm·(1 - α + 4α/π)`. Cross-checked against the C reference
    /// value 0.46557748 for α=0.6.
    #[test]
    fn center_tap_matches_c_reference() {
        // Prototype is NUM_COEFFS=325 long; center index = 162.
        let c = rrc_coeff(
            NUM_COEFFS / 2,
            NUM_COEFFS,
            2.0 * INTERP_FACTOR as f32,
            ROLLOFF,
        );
        let expected = (2.0 / 5.0) * (1.0 - 0.6 + 4.0 * 0.6 / PI);
        assert!(
            (c - expected).abs() < 1e-6,
            "center tap {c} != expected {expected}",
        );
        assert!(
            (c - 0.465_577_5).abs() < 1e-4,
            "center tap {c} should match C reference 0.4655775",
        );
    }

    /// The full 325-tap prototype sums to ~4.0 (= `INTERP_FACTOR` ×
    /// the 0.8 DC gain of the equivalent factor=1 65-tap filter the
    /// C reference's non-interpolating config produces), confirming
    /// the window and `norm = 2/5` are applied correctly across all
    /// 325 prototype taps.
    #[test]
    #[allow(clippy::cast_precision_loss)]
    fn prototype_sum_matches_c_reference() {
        let osf = 2.0 * INTERP_FACTOR as f32;
        let sum: f32 = (0..NUM_COEFFS)
            .map(|n| rrc_coeff(n, NUM_COEFFS, osf, ROLLOFF))
            .sum();
        assert!(
            (sum - INTERP_FACTOR as f32 * 0.8).abs() < 1e-2,
            "prototype coefficient sum {sum} should be ~{} (5 × 0.8)",
            INTERP_FACTOR as f32 * 0.8,
        );
    }

    /// Each polyphase sub-filter is symmetric only as a whole
    /// prototype; here we pin that the edge prototype taps are
    /// near-zero (windowed), matching C tap[0] ≈ 9.6e-22.
    #[test]
    fn prototype_edges_are_windowed_to_near_zero() {
        let osf = 2.0 * INTERP_FACTOR as f32;
        let t0 = rrc_coeff(0, NUM_COEFFS, osf, ROLLOFF);
        assert!(
            t0.abs() < 1e-3,
            "edge tap {t0} should be windowed near zero"
        );
    }

    /// A populated history yields a finite, non-NaN filtered output
    /// for every polyphase phase.
    #[test]
    fn get_is_finite_for_all_phases() {
        let mut f = RrcFilter::new(2.0);
        for n in 0..NUM_TAPS {
            #[allow(clippy::cast_precision_loss)]
            let v = (n as f32 * 0.1).sin();
            f.push(Complex::new(v, -v));
        }
        for phase in 0..INTERP_FACTOR {
            let out = f.get(phase);
            assert!(
                out.re.is_finite() && out.im.is_finite(),
                "phase {phase} non-finite"
            );
        }
    }
}
