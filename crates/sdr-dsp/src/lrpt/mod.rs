//! Meteor-M LRPT QPSK / OQPSK demodulator (epic #469 + issue #662).
//!
//! Faithful transliteration of dbdexter `meteor_demod` — the
//! reference demodulator that produces the soft-symbol `.s` stream
//! the FEC chain decodes. Both modulation modes run the **same**
//! signal chain, dispatched from [`LrptDemod`] by mode and matching
//! `demod.c::demod_qpsk` / `demod.c::demod_oqpsk` line-for-line:
//!
//! ```text
//!   push input → polyphase RRC (interp ×5) → [per fired timeslot]
//!     RrcFilter::get(phase) → MeteorAgc → MeteorPll mix → MmTiming
//!     retime + MeteorPll update_estimate → soft pair (clamp re/2)
//! ```
//!
//! - **QPSK** (legacy METEOR-M N2): one timeslot tick per symbol
//!   ([`MmTiming::advance_timeslot`]); the full complex mix is
//!   retimed/sliced. `demod.c::demod_qpsk`.
//! - **OQPSK** (current METEOR-M2 3 / METEOR-M2 4): two ticks per
//!   symbol ([`MmTiming::advance_timeslot_dual`]) — the I rail is
//!   captured on the state-1 tick and the Q rail half a symbol later
//!   on the state-2 tick (offset QPSK). `demod.c::demod_oqpsk`.
//!
//! The polyphase RRC ([`RrcFilter`]) is both the matched filter and
//! the fractional-delay interpolator the timing loop steers
//! (`INTERP_FACTOR` = 5 sub-phases per input sample = 1/5 = 0.2 of
//! an input sample, or 1/10 = 0.1 of a symbol at 2 sps), so the M&M
//! loop has 0.1-symbol timing authority rather than the 1-sample
//! authority a single-rate filter would give. The AGC, PLL, and
//! Mueller-Muller modules are exact ports of `dsp/{agc,pll,timing}.c`.
//!
//! References (read-only): `original/meteor_demod/demod.c`,
//! `original/meteor_demod/demod.h`, `original/meteor_demod/main.c`
//! (soft-symbol output format), and `dsp/{filter,agc,pll,timing}.c`.

pub mod meteor_agc;
pub mod meteor_pll;
pub mod mm_timing;
pub mod rrc_filter;

pub use meteor_agc::MeteorAgc;
pub use meteor_pll::MeteorPll;
pub use mm_timing::MmTiming;
pub use rrc_filter::{INTERP_FACTOR, RrcFilter};

use sdr_types::{Complex, DspError};

/// Meteor LRPT symbol rate (symbols per second). `demod.h:9`
/// (`SYM_RATE 72000.0`).
pub const SYMBOL_RATE_HZ: f32 = 72_000.0;

/// Working sample rate for the demod chain. The upstream VFO
/// delivers exactly 2 samples per symbol (the standard QPSK
/// convention); the polyphase RRC interpolates ×[`INTERP_FACTOR`]
/// internally from there.
pub const SAMPLE_RATE_HZ: f32 = SYMBOL_RATE_HZ * 2.0;

/// Oversampling factor handed to the RRC design: input
/// `samplerate / symrate`. `demod.c:14` passes
/// `(float)samplerate/symrate`; here a fixed 2.0.
const OSF: f32 = SAMPLE_RATE_HZ / SYMBOL_RATE_HZ;

/// dbdexter `pll_bw` default — 1 Hz effective loop bandwidth.
/// `demod.h:15` (`PLL_BW 1`).
const DBDEXTER_PLL_BW_HZ: f32 = 1.0;

/// dbdexter `SYM_BW` — Mueller-Muller loop bandwidth before the
/// per-interp division. `demod.h:14` (`SYM_BW 0.00005`).
const DBDEXTER_SYM_BW: f32 = 0.000_05;

/// QPSK carrier-PLL loop bandwidth, radians per `mix` call.
/// `demod.c:12`: `2π·pll_bw/(multiplier·symrate)` with
/// `multiplier = 2` for QPSK (`demod.c:10`).
const QPSK_PLL_BW: f32 = 2.0 * core::f32::consts::PI * DBDEXTER_PLL_BW_HZ / (2.0 * SYMBOL_RATE_HZ);

/// OQPSK carrier-PLL loop bandwidth, radians per `mix_*` call.
/// `demod.c:12` with `multiplier = 1` for OQPSK.
const OQPSK_PLL_BW: f32 = 2.0 * core::f32::consts::PI * DBDEXTER_PLL_BW_HZ / SYMBOL_RATE_HZ;

/// Mueller-Muller initial symbol period, radians per timeslot tick.
/// `demod.c:13`: `2π·symrate/(samplerate·interp_factor)`. At 2 sps
/// and interp 5 this is `2π·72000/(144000·5) = π/5`.
#[allow(
    clippy::cast_precision_loss,
    reason = "INTERP_FACTOR (= 5) converts to f32 exactly"
)]
const MM_SYM_FREQ: f32 =
    2.0 * core::f32::consts::PI * SYMBOL_RATE_HZ / (SAMPLE_RATE_HZ * INTERP_FACTOR as f32);

/// Mueller-Muller loop bandwidth. `demod.c:13`: `sym_bw/interp_factor`
/// = `0.00005/5 = 1e-5`. The `/interp_factor` is required because the
/// phase accumulator advances `INTERP_FACTOR` times per input sample.
#[allow(
    clippy::cast_precision_loss,
    reason = "INTERP_FACTOR (= 5) converts to f32 exactly"
)]
const MM_TIMING_BW: f32 = DBDEXTER_SYM_BW / INTERP_FACTOR as f32;

/// Modulation modes supported by [`LrptDemod`]. The catalog layer
/// (`sdr-sat`) carries its own equivalent enum — the controller is
/// the seam that maps from one to the other.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LrptMode {
    /// Standard QPSK. Used by legacy METEOR-M N2 recordings.
    Qpsk,
    /// Offset QPSK — Q delayed by Tsym/2 from I. Used by
    /// METEOR-M2 3 and METEOR-M2 4.
    Oqpsk,
}

/// Top-level LRPT demodulator chain. The mode is picked at
/// construction (defaulting to QPSK for backward compatibility);
/// `process()` dispatches each input sample down the appropriate
/// inner pipeline.
pub struct LrptDemod {
    rrc: RrcFilter,
    inner: DemodInner,
}

/// Per-mode demod state. Both variants share the dbdexter
/// AGC/PLL/MM machinery; the OQPSK variant additionally stashes the
/// in-phase rail between its two per-symbol timeslot ticks.
enum DemodInner {
    Qpsk {
        agc: MeteorAgc,
        pll: MeteorPll,
        timing: MmTiming,
    },
    Oqpsk {
        agc: MeteorAgc,
        pll: MeteorPll,
        timing: MmTiming,
        /// In-phase sample stashed between the I-tick and the Q-tick.
        /// `demod.c::demod_oqpsk`'s `static float inphase`.
        pending_i: f32,
    },
}

impl LrptDemod {
    /// Build a QPSK demod chain. Equivalent to
    /// `new_with_mode(LrptMode::Qpsk)` — kept as a no-arg
    /// constructor for backward compatibility with the call sites
    /// that predate the modulation enum.
    ///
    /// # Errors
    ///
    /// Returns `DspError::InvalidParameter` if an inner loop rejects
    /// its synthesized parameters (practically unreachable for the
    /// pinned constants in this module).
    pub fn new() -> Result<Self, DspError> {
        Self::new_with_mode(LrptMode::Qpsk)
    }

    /// Build a demod chain in the requested mode. Both modes use the
    /// dbdexter AGC + [`MeteorPll`] + [`MmTiming`] loops; QPSK and
    /// OQPSK differ only in the carrier loop bandwidth multiplier,
    /// the PLL free-run half-bandwidth, and the timeslot machine.
    ///
    /// # Errors
    ///
    /// Returns `DspError::InvalidParameter` if any inner loop rejects
    /// its parameters. Practically unreachable for the pinned
    /// constants in this module.
    pub fn new_with_mode(mode: LrptMode) -> Result<Self, DspError> {
        let inner = match mode {
            LrptMode::Qpsk => DemodInner::Qpsk {
                agc: MeteorAgc::new(),
                pll: MeteorPll::new(QPSK_PLL_BW, false, None)?,
                timing: MmTiming::new(MM_SYM_FREQ, MM_TIMING_BW)?,
            },
            LrptMode::Oqpsk => DemodInner::Oqpsk {
                agc: MeteorAgc::new(),
                pll: MeteorPll::new(OQPSK_PLL_BW, true, None)?,
                timing: MmTiming::new(MM_SYM_FREQ, MM_TIMING_BW)?,
                pending_i: 0.0,
            },
        };
        Ok(Self {
            rrc: RrcFilter::new(OSF)?,
            inner,
        })
    }

    /// Whether the carrier-recovery PLL has ever locked since
    /// construction. Exposed for diagnostics and the `oqpsk_zero_iq_*`
    /// regression tests. Per CR round 2 on PR #663.
    #[must_use]
    pub fn locked_once(&self) -> bool {
        match &self.inner {
            DemodInner::Qpsk { pll, .. } | DemodInner::Oqpsk { pll, .. } => pll.locked_once(),
        }
    }

    /// Back-compat alias for [`Self::locked_once`]. Returns
    /// `Some(locked)` always (both modes now expose a PLL lock
    /// detector); kept `Option`-typed for the existing call sites.
    #[must_use]
    pub fn oqpsk_locked_once(&self) -> Option<bool> {
        Some(self.locked_once())
    }

    /// Push one complex baseband sample. Returns up to one soft-
    /// symbol pair `[i, q]` when the timing recovery fires a symbol
    /// tick. Transliterates the per-sample body of
    /// `demod.c::demod_qpsk` / `demod.c::demod_oqpsk`: push the input
    /// into the polyphase RRC once, then walk the `INTERP_FACTOR`
    /// sub-phases, doing AGC + carrier mix + retime only on the
    /// timeslots the M&M loop fires.
    pub fn process(&mut self, x: Complex) -> Option<[i8; 2]> {
        let Self { rrc, inner } = self;
        // demod.c:29 / :58 — filter_fwd_sample(&_rrc_filter, *sample);
        rrc.push(x);

        let mut emitted: Option<[i8; 2]> = None;
        match inner {
            DemodInner::Qpsk { agc, pll, timing } => {
                // demod.c:33-45 — for i in 0..interp_factor { if advance_timeslot() {...} }
                for phase in 0..INTERP_FACTOR {
                    if timing.advance_timeslot() {
                        // phase is always < INTERP_FACTOR here, so get() is Some.
                        let Some(mut out) = rrc.get(phase) else {
                            continue;
                        }; // filter_get(flt, i)
                        out = agc.process(out); // agc_apply(out)
                        out = pll.mix(out); // pll_mix(out)
                        timing.retime(out); // retime(out)
                        pll.update_estimate(out.re, out.im); // pll_update_estimate(re, im)
                        emitted = Some(soft_pair(out)); // *sample = out
                    }
                }
            }
            DemodInner::Oqpsk {
                agc,
                pll,
                timing,
                pending_i,
            } => {
                // demod.c:62-88 — for i in 0..interp_factor { switch advance_timeslot_dual() }
                for phase in 0..INTERP_FACTOR {
                    match timing.advance_timeslot_dual() {
                        1 => {
                            // demod.c:66-71 — intersample: capture the I rail only.
                            let Some(s) = rrc.get(phase) else { continue };
                            let out = agc.process(s);
                            *pending_i = pll.mix_i(out);
                        }
                        2 => {
                            // demod.c:72-83 — actual sample: capture Q, reassemble
                            // I/Q, retime, update carrier, emit.
                            let Some(s) = rrc.get(phase) else { continue };
                            let out = agc.process(s);
                            let quad = pll.mix_q(out);
                            let symbol = Complex::new(*pending_i, quad);
                            timing.retime(symbol);
                            pll.update_estimate(*pending_i, quad);
                            emitted = Some(soft_pair(symbol));
                        }
                        _ => {}
                    }
                }
            }
        }
        emitted
    }
}

/// Convert a demod-output symbol to the soft `[i8; 2]` pair the FEC
/// chain consumes, exactly as `main.c:305-306` writes the `.s`
/// stream: `clamp(re/2, ±127)`, `clamp(im/2, ±127)` with the C
/// `int8_t` truncating cast.
#[allow(
    clippy::cast_possible_truncation,
    reason = "value is clamped to [-127, 127] before the truncating cast, mirroring C (int8_t)"
)]
fn soft_pair(sample: Complex) -> [i8; 2] {
    let q = |v: f32| (v / 2.0).clamp(-127.0, 127.0) as i8;
    [q(sample.re), q(sample.im)]
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;

    #[test]
    fn qpsk_pipeline_produces_soft_symbols_from_synthetic_qpsk() {
        // Synthesize ~2 sps QPSK with no impairments. Pipeline
        // converges and emits signed i8 pairs at roughly one per two
        // input samples (post RRC + timing warmup).
        let mut demod = LrptDemod::new().expect("LrptDemod::new");
        let symbols = [
            Complex::new(0.707, 0.707),
            Complex::new(-0.707, 0.707),
            Complex::new(0.707, -0.707),
            Complex::new(-0.707, -0.707),
        ];
        let mut emitted = 0_usize;
        for n in 0..8000 {
            let sym = symbols[(n / 2) % 4];
            let s = if n % 2 == 0 {
                sym
            } else {
                Complex::new(0.0, 0.0)
            };
            if demod.process(s).is_some() {
                emitted += 1;
            }
        }
        // 8000 inputs at 2 sps → ~4000 emitted after warmup.
        assert!(
            emitted > 3000,
            "QPSK pipeline should emit ~4000 soft symbols, got {emitted}",
        );
    }

    #[test]
    fn oqpsk_pipeline_produces_soft_symbols_from_synthetic_oqpsk() {
        // Synthesize 2 sps OQPSK by interleaving I-only and Q-only
        // samples on alternating indices ("Q delayed by Tsym/2").
        let mut demod =
            LrptDemod::new_with_mode(LrptMode::Oqpsk).expect("LrptDemod::new_with_mode");
        let i_vals = [0.707_f32, -0.707, 0.707, -0.707];
        let q_vals = [0.707_f32, 0.707, -0.707, -0.707];
        let mut emitted = 0_usize;
        for n in 0..8000 {
            let sym_idx = (n / 2) % 4;
            let s = if n % 2 == 0 {
                Complex::new(i_vals[sym_idx], 0.0)
            } else {
                Complex::new(0.0, q_vals[sym_idx])
            };
            if demod.process(s).is_some() {
                emitted += 1;
            }
        }
        assert!(
            emitted > 3000,
            "OQPSK pipeline should emit ~4000 soft symbols, got {emitted}",
        );
    }

    /// Build `sps=2`, β=0.6 RRC transmit pulse taps (energy-norm),
    /// the matched twin of the demod's receive RRC.
    fn tx_rrc_taps() -> Vec<f32> {
        use core::f32::consts::PI;
        let beta = 0.6_f32;
        let sps = 2.0_f32;
        let half = 32_i32;
        let mut taps = Vec::new();
        for n in -half..=half {
            #[allow(clippy::cast_precision_loss)]
            let t = n as f32 / sps;
            let v = if t.abs() < 1e-6 {
                1.0 - beta + 4.0 * beta / PI
            } else {
                let d = 1.0 - (4.0 * beta * t) * (4.0 * beta * t);
                if d.abs() < 1e-6 {
                    (beta / 2.0_f32.sqrt())
                        * ((1.0 + 2.0 / PI) * (PI / (4.0 * beta)).sin()
                            + (1.0 - 2.0 / PI) * (PI / (4.0 * beta)).cos())
                } else {
                    ((PI * t * (1.0 - beta)).sin() + 4.0 * beta * t * (PI * t * (1.0 + beta)).cos())
                        / (PI * t * d)
                }
            };
            taps.push(v);
        }
        let e: f32 = taps.iter().map(|x| x * x).sum::<f32>().sqrt();
        for x in &mut taps {
            *x /= e;
        }
        taps
    }

    /// Modulate hard QPSK symbols into a clean 2-sps baseband by
    /// RRC pulse shaping (the inverse of what the demod does).
    fn modulate(syms: &[Complex], taps: &[f32]) -> Vec<Complex> {
        let sps = 2;
        let mut up = vec![Complex::new(0.0, 0.0); syms.len() * sps];
        for (k, s) in syms.iter().enumerate() {
            up[k * sps] = *s;
        }
        // Convolve with the real-valued RRC taps.
        let mut out = vec![Complex::new(0.0, 0.0); up.len()];
        let m = taps.len();
        for (i, o) in out.iter_mut().enumerate() {
            let mut acc = Complex::new(0.0, 0.0);
            for (j, &h) in taps.iter().enumerate() {
                if i + j >= m - 1 {
                    let idx = i + j - (m - 1);
                    if idx < up.len() {
                        acc += up[idx] * h;
                    }
                }
            }
            *o = acc;
        }
        out
    }

    /// Best sign agreement between aligned recovered (`rec`) and
    /// transmitted (`tx`) symbol slices over the four QPSK phase
    /// rotations and the I/Q-swap conjugate — the demod resolves
    /// phase only up to the 8-fold ambiguity the downstream FEC
    /// rotation search handles. Slices are already index-aligned.
    fn sign_agreement(rec: &[[i8; 2]], tx: &[Complex]) -> f32 {
        let n = rec.len().min(tx.len());
        let mut best = 0.0_f32;
        for k in 0..4 {
            for conj in [false, true] {
                let mut ok = 0_usize;
                for j in 0..n {
                    let r = rec[j];
                    let (mut ri, mut rq) = (f32::from(r[0]), f32::from(r[1]));
                    if conj {
                        rq = -rq;
                    }
                    // rotate by k*90° clockwise
                    for _ in 0..k {
                        let (ni, nq) = (-rq, ri);
                        ri = ni;
                        rq = nq;
                    }
                    if (ri >= 0.0) == (tx[j].re >= 0.0) {
                        ok += 1;
                    }
                    if (rq >= 0.0) == (tx[j].im >= 0.0) {
                        ok += 1;
                    }
                }
                #[allow(clippy::cast_precision_loss)]
                let a = ok as f32 / (2 * n) as f32;
                best = best.max(a);
            }
        }
        best
    }

    #[test]
    fn demod_recovers_clean_qpsk_symbols() {
        // Faithful-port regression guard: a clean RRC-shaped QPSK
        // signal must demodulate back to (essentially) the exact
        // transmitted symbols. Mirrors the external validation that
        // measured SER ≈ 0 on clean QPSK (issue #566 demod work).
        let n_syms = 6000;
        // Deterministic pseudo-random QPSK symbols (no rng dep).
        let mut state: u32 = 0x1234_5678;
        let mut syms = Vec::with_capacity(n_syms);
        for _ in 0..n_syms {
            state = state.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
            let i = if state & 1 == 0 { 1.0 } else { -1.0 };
            let q = if state & 2 == 0 { 1.0 } else { -1.0 };
            syms.push(Complex::new(i, q));
        }
        let taps = tx_rrc_taps();
        let iq = modulate(&syms, &taps);
        let mut demod = LrptDemod::new().expect("LrptDemod::new");
        let rec: Vec<[i8; 2]> = iq.iter().filter_map(|&s| demod.process(s)).collect();
        assert!(
            rec.len() > n_syms - 200,
            "expected ~{n_syms} symbols, got {}",
            rec.len()
        );
        // Measure agreement over a settled tail window, searching the
        // small group-delay offset: recovered[r_start + j] aligns with
        // transmitted[r_start - delay + j].
        let n = 2000;
        let r_start = rec.len() - n;
        let mut best = 0.0_f32;
        for delay in 0..120 {
            if delay > r_start || r_start - delay + n > syms.len() {
                continue;
            }
            let s_start = r_start - delay;
            best = best.max(sign_agreement(
                &rec[r_start..r_start + n],
                &syms[s_start..s_start + n],
            ));
        }
        assert!(
            best > 0.97,
            "clean QPSK demod should recover symbols (agreement {best:.4}, want > 0.97)",
        );
    }

    #[test]
    fn oqpsk_constructor_succeeds() {
        assert!(LrptDemod::new_with_mode(LrptMode::Oqpsk).is_ok());
    }

    #[test]
    fn qpsk_constructor_succeeds() {
        assert!(LrptDemod::new_with_mode(LrptMode::Qpsk).is_ok());
    }

    #[test]
    fn zero_iq_never_acquires_lock_oqpsk() {
        // Regression for CR round 2 on PR #663. The AGC + lock
        // detector's signal-magnitude floor must keep the PLL from
        // false-locking on silence.
        let mut demod = LrptDemod::new_with_mode(LrptMode::Oqpsk).unwrap();
        for _ in 0..100_000 {
            demod.process(Complex::default());
        }
        assert!(
            !demod.locked_once(),
            "zero-IQ silence must not trigger the lock detector",
        );
    }

    #[test]
    fn processes_zero_iq_without_panicking() {
        let mut demod = LrptDemod::new_with_mode(LrptMode::Oqpsk).unwrap();
        for _ in 0..1000 {
            let _ = demod.process(Complex::default());
        }
    }
}
