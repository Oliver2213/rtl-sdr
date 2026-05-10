//! Mueller-Muller symbol-timing recovery — faithful port of
//! `dbdexter/meteor_demod/dsp/timing.c`.
//!
//! Mueller-Muller (M&M) is a single-rail timing-error metric:
//!
//! ```text
//! err = sgn(prev) * cur − sgn(cur) * prev
//! ```
//!
//! computed on the imaginary part of the recovered symbol. It's
//! non-data-aided (uses the sign-decision as the local symbol
//! estimate), needs only one sample per symbol, and locks faster
//! than Gardner on QPSK / OQPSK because the dual-rail Gardner
//! error metric assumes I and Q are co-timed — which is exactly
//! the assumption OQPSK breaks.
//!
//! Two timeslot variants drive the loop:
//!
//! - [`Self::advance_timeslot`] — one tick per symbol, used by
//!   QPSK. Returns `true` on the symbol boundary so the caller
//!   knows to mix + retime + update.
//! - [`Self::advance_timeslot_dual`] — two ticks per symbol,
//!   alternating `1 → 2 → 1 → 2`. State 1 is the I-tick (mix the
//!   in-phase rail half a symbol before the symbol boundary);
//!   state 2 is the Q-tick (mix the quadrature rail at the symbol
//!   boundary, retime, update PLL). Used by OQPSK.
//!
//! The loop is 2nd-order with critically-damped (damping = 1) loop
//! coefficients — same shape as the PLL, just a different damping
//! constant.

use core::f32::consts::PI;

use sdr_types::{Complex, DspError};

/// dbdexter (`timing.c:7`): `freq` is allowed to deviate by at
/// most `±2^-FREQ_DEV_EXP` × `center_freq` from the configured
/// symbol period before the loop hard-clamps. With `FREQ_DEV_EXP =
/// 12` that's ±0.024% — tight enough that the loop tracks symbol-
/// clock drift but not so tight that initial acquisition is
/// throttled.
const FREQ_DEV_EXP: u32 = 12;

/// Mueller-Muller timing recovery state.
pub struct MmTiming {
    /// Sample-clock phase accumulator. Increments by `freq` each
    /// `advance_timeslot*` call; the timeslot fires when `phase`
    /// crosses the configured threshold (`2π` for single, `state·π`
    /// for dual).
    phase: f32,
    /// Current symbol period estimate, in radians per
    /// `advance_timeslot*` call. Tracked by the loop.
    freq: f32,
    /// Configured nominal symbol period — `freq` is clamped to
    /// `center_freq ± freq_max_dev`.
    center_freq: f32,
    /// Maximum allowed deviation of `freq` from `center_freq`
    /// (= `center_freq / 2^FREQ_DEV_EXP`).
    freq_max_dev: f32,
    /// Loop-filter proportional gain.
    alpha: f32,
    /// Loop-filter integral gain.
    beta: f32,
    /// Imaginary part of the previously-decimated symbol — the
    /// `prev` term in the M&M error metric.
    prev: f32,
    /// dbdexter `advance_timeslot_dual` (`timing.c:43`) state
    /// machine: alternates `1 → 2 → 1 → 2`. State 1 returns on
    /// `phase >= π` (I-tick); state 2 returns on `phase >= 2π`
    /// (Q-tick / symbol-boundary).
    state: u8,
}

impl MmTiming {
    /// Build an M&M timing loop.
    ///
    /// - `sym_freq` — expected symbol period, in radians per
    ///   `advance_timeslot*` call. For Meteor at 2 sps that's
    ///   `2π × 72_000 / 144_000 = π`.
    /// - `bandwidth` — loop bandwidth (dimensionless). dbdexter's
    ///   default `SYM_BW = 0.00005` (`demod.h:14`) is a good
    ///   starting point.
    ///
    /// # Errors
    ///
    /// Returns `DspError::InvalidParameter` if either input is not
    /// finite or not positive.
    pub fn new(sym_freq: f32, bandwidth: f32) -> Result<Self, DspError> {
        if !sym_freq.is_finite() || sym_freq <= 0.0 {
            return Err(DspError::InvalidParameter(format!(
                "sym_freq must be finite and positive, got {sym_freq}"
            )));
        }
        if !bandwidth.is_finite() || bandwidth <= 0.0 {
            return Err(DspError::InvalidParameter(format!(
                "bandwidth must be finite and positive, got {bandwidth}"
            )));
        }
        #[allow(
            clippy::cast_precision_loss,
            reason = "FREQ_DEV_EXP < 24 so 1u32 << FREQ_DEV_EXP is exactly representable as f32"
        )]
        let freq_max_dev = sym_freq / ((1u32 << FREQ_DEV_EXP) as f32);
        // dbdexter (`timing.c:97`): `update_alpha_beta(1, bw)` —
        // damping = 1 here, *different* from the PLL's
        // damping = 1/√2.
        let damping = 1.0_f32;
        let denom = 1.0 + 2.0 * damping * bandwidth + bandwidth * bandwidth;
        let alpha = 4.0 * damping * bandwidth / denom;
        let beta = 4.0 * bandwidth * bandwidth / denom;
        Ok(Self {
            phase: 0.0,
            freq: sym_freq,
            center_freq: sym_freq,
            freq_max_dev,
            alpha,
            beta,
            prev: 0.0,
            state: 1,
        })
    }

    /// QPSK timeslot tick. Advances the phase accumulator by one
    /// `freq` step and returns `true` on the symbol boundary
    /// (`phase >= 2π`). Per dbdexter (`timing.c:32`).
    ///
    /// The phase is intentionally not reset here — `retime` does
    /// the symbol-boundary subtraction, so the loop-filter sees
    /// the residual error.
    pub fn advance_timeslot(&mut self) -> bool {
        self.phase += self.freq;
        self.phase >= 2.0 * PI
    }

    /// OQPSK timeslot tick. Advances the phase accumulator and
    /// returns:
    ///
    /// - `0` — no tick this sample.
    /// - `1` — I-tick: mix the in-phase rail (`mix_i`) and stash
    ///   the result. State machine flips to expecting state 2
    ///   next.
    /// - `2` — Q-tick: mix the quadrature rail (`mix_q`),
    ///   reassemble the I/Q pair, retime, update the PLL. State
    ///   machine flips to expecting state 1 next.
    ///
    /// Per dbdexter (`timing.c:41`).
    pub fn advance_timeslot_dual(&mut self) -> u8 {
        self.phase += self.freq;
        let threshold = f32::from(self.state) * PI;
        if self.phase >= threshold {
            let ret = self.state;
            // dbdexter (`timing.c:52`): `state = (state % 2) + 1`
            // — toggles 1 ↔ 2.
            self.state = if self.state == 1 { 2 } else { 1 };
            return ret;
        }
        0
    }

    /// Update the timing estimate from one decimated complex
    /// symbol. Uses only the imaginary part — M&M is single-rail.
    pub fn retime(&mut self, sample: Complex) {
        let cur = sample.im;
        let err = sgn(self.prev) * cur - sgn(cur) * self.prev;
        self.prev = cur;
        self.apply_loop_update(err);
    }

    /// Current symbol-period estimate, in radians per
    /// `advance_timeslot*` call. Useful for diagnostics — at lock
    /// it should sit very close to the configured `sym_freq`.
    pub fn omega(&self) -> f32 {
        self.freq
    }

    fn apply_loop_update(&mut self, error: f32) {
        let mut freq_delta = self.freq - self.center_freq;
        // dbdexter (`timing.c:80`): `_phase -= 2*M_PI + _alpha*error`.
        // The `2π` is the symbol-boundary subtraction (so the next
        // tick fires after another full symbol period); the
        // `alpha*error` term is the proportional correction.
        self.phase -= 2.0 * PI + self.alpha * error;
        freq_delta -= self.beta * error;
        freq_delta = freq_delta.clamp(-self.freq_max_dev, self.freq_max_dev);
        self.freq = self.center_freq + freq_delta;
    }
}

#[inline]
fn sgn(x: f32) -> f32 {
    if x > 0.0 {
        1.0
    } else if x < 0.0 {
        -1.0
    } else {
        0.0
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;

    /// A representative loop bandwidth for the tests. Wider than
    /// dbdexter's default 0.00005 so unit tests converge in a
    /// few thousand samples instead of tens of thousands.
    const TEST_BW: f32 = 0.005;

    #[test]
    fn rejects_invalid_inputs() {
        assert!(MmTiming::new(0.0, TEST_BW).is_err());
        assert!(MmTiming::new(-1.0, TEST_BW).is_err());
        assert!(MmTiming::new(f32::NAN, TEST_BW).is_err());
        assert!(MmTiming::new(PI, 0.0).is_err());
        assert!(MmTiming::new(PI, -1.0).is_err());
        assert!(MmTiming::new(PI, f32::INFINITY).is_err());
    }

    #[test]
    fn sgn_handles_zero() {
        assert!((sgn(0.0)).abs() < 1e-9);
        assert!((sgn(0.5) - 1.0).abs() < 1e-9);
        assert!((sgn(-0.5) - (-1.0)).abs() < 1e-9);
    }

    #[test]
    fn advance_timeslot_fires_every_two_samples_at_sps_2() {
        // sym_freq = π → phase += π each call → fires (>= 2π) on
        // every second call.
        let mut t = MmTiming::new(PI, TEST_BW).unwrap();
        let mut fires = 0_usize;
        for _ in 0..100 {
            if t.advance_timeslot() {
                fires += 1;
                // Reset the phase manually here — the real demod
                // loop calls `retime` which subtracts 2π. Without
                // that, every call past the first would fire.
                // Simulate the subtraction.
                t.phase -= 2.0 * PI;
            }
        }
        assert_eq!(fires, 50, "expected 50 fires from 100 samples at sps=2");
    }

    #[test]
    fn advance_timeslot_dual_alternates_state_1_then_2() {
        // sym_freq = π → phase crosses π after one sample (state 1
        // tick), 2π after two (state 2 tick). After the state 2
        // tick the symbol-boundary subtraction (via `retime`) takes
        // phase back to ~0, and the cycle repeats.
        let mut t = MmTiming::new(PI, TEST_BW).unwrap();
        let mut history = Vec::new();
        for _ in 0..20 {
            let r = t.advance_timeslot_dual();
            history.push(r);
            // Simulate `retime`'s symbol-boundary subtraction so
            // we don't keep firing forever.
            if r == 2 {
                t.phase -= 2.0 * PI;
            }
        }
        // Expected pattern: every second call yields a tick. The
        // first sample brings phase from 0 to π → state 1 tick;
        // the next brings it to 2π → state 2 tick; then we
        // subtract 2π and repeat.
        let ticks: Vec<u8> = history.into_iter().filter(|&r| r != 0).collect();
        assert_eq!(ticks.len(), 20, "expected one tick per sample");
        for (i, &r) in ticks.iter().enumerate() {
            let expected = if i % 2 == 0 { 1 } else { 2 };
            assert_eq!(r, expected, "tick {i} expected state {expected}, got {r}");
        }
    }

    #[test]
    fn retime_zero_error_at_perfect_constellation() {
        // Mueller-Muller error: sgn(prev)*cur - sgn(cur)*prev.
        // When prev == cur both terms cancel → error = 0 → no
        // freq update.
        let mut t = MmTiming::new(PI, TEST_BW).unwrap();
        let s = Complex::new(0.0, 1.0);
        t.retime(s); // prev = 1.0
        let omega_before = t.omega();
        // Drive phase up so retime's `-= 2π + alpha*error` doesn't
        // wrap to a wildly different state.
        t.phase = 2.0 * PI;
        t.retime(s); // err should be 0
        // Center-freq clamp means freq returns to center if delta == 0;
        // omega should equal center_freq.
        assert!(
            (t.omega() - omega_before).abs() < 1e-9,
            "zero-error retime must not perturb omega"
        );
    }

    #[test]
    fn freq_clamped_to_max_dev() {
        let mut t = MmTiming::new(PI, TEST_BW).unwrap();
        // Force-feed a huge error directly to apply_loop_update.
        for _ in 0..10_000 {
            t.apply_loop_update(1000.0);
        }
        let max_freq = t.center_freq + t.freq_max_dev;
        let min_freq = t.center_freq - t.freq_max_dev;
        assert!(
            t.freq <= max_freq && t.freq >= min_freq,
            "freq {} out of clamp range [{}, {}]",
            t.freq,
            min_freq,
            max_freq
        );
    }
}
