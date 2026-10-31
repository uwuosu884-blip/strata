//! Smooth playhead clock.
//!
//! The engine publishes its position once per audio callback (every 5–40 ms
//! depending on the buffer size), and the UI samples it at whatever moment a
//! frame happens to start, so the raw value advances in uneven steps and the
//! playhead stutters. This clock advances with wall time at the current tempo
//! every frame and is gently pulled towards the engine's position; jumps
//! (start, locate, loop wrap, tempo change) snap straight to the engine.

use std::time::Instant;

/// Further apart than this (seconds of music) = a jump, not jitter.
const SNAP_S: f64 = 0.15;
/// Fraction of the remaining error corrected per frame.
const PULL: f64 = 0.12;
/// How far past the last engine update the estimate may run (seconds), so a
/// stalled engine (count-in, xrun) doesn't let the playhead run away.
const MAX_AHEAD_S: f64 = 0.1;

#[derive(Debug, Clone)]
pub struct PlayClock {
    raw: f64,
    raw_at: Instant,
    shown: f64,
    last: Instant,
    playing: bool,
}

impl PlayClock {
    pub fn new(now: Instant) -> Self {
        Self { raw: 0.0, raw_at: now, shown: 0.0, last: now, playing: false }
    }

    /// Beat position to draw this frame. `lp` = active loop (start, end).
    pub fn update(&mut self, raw: f64, playing: bool, tempo: f64, lp: Option<(f64, f64)>, now: Instant) -> f64 {
        let dt = now.saturating_duration_since(self.last).as_secs_f64().min(0.25);
        self.last = now;
        if raw != self.raw {
            self.raw = raw;
            self.raw_at = now;
        }
        if !playing || !self.playing || tempo <= 0.0 {
            self.playing = playing;
            self.shown = raw;
            return raw;
        }
        let bps = tempo / 60.0;
        // Where the engine most likely is now: its last value plus the time since.
        let since = now.saturating_duration_since(self.raw_at).as_secs_f64().min(MAX_AHEAD_S);
        let mut target = raw + since * bps;
        let mut pred = self.shown + dt * bps;
        let mut wrapped = false;
        if let Some((a, b)) = lp
            && b > a
            && raw < b
        {
            // The engine wraps at the loop end; so does the estimate.
            if target >= b {
                target -= b - a;
            }
            if pred >= b {
                pred -= b - a;
                wrapped = true;
            }
        }
        let err = target - pred;
        let next = pred + err * PULL;
        self.shown = if err.abs() > SNAP_S * bps {
            target
        } else if !wrapped && next < self.shown {
            // Never step backwards for small errors (that reads as a stutter too).
            self.shown
        } else {
            next
        };
        self.shown
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Duration;

    /// Engine updates every 21.3 ms (1024 frames at 48 kHz), UI at 60 fps:
    /// the clock's per-frame steps must be even while the raw steps are not.
    #[test]
    fn steps_are_even_with_coarse_engine_updates() {
        let t0 = Instant::now();
        let mut c = PlayClock::new(t0);
        let tempo = 120.0;
        let block = 1024.0 / 48000.0;
        let raw_at = |t: f64| (t / block).floor() * block * tempo / 60.0;
        c.update(0.0, true, tempo, None, t0);
        let mut prev = c.update(0.0, true, tempo, None, t0);
        let mut steps = Vec::new();
        for f in 1..240 {
            let t = f as f64 / 60.0;
            let v = c.update(raw_at(t), true, tempo, None, t0 + Duration::from_secs_f64(t));
            if f > 60 {
                steps.push(v - prev);
            }
            prev = v;
        }
        let ideal = tempo / 60.0 / 60.0;
        let worst = steps.iter().map(|s| (s - ideal).abs() / ideal).fold(0.0, f64::max);
        assert!(worst < 0.35, "worst step deviation {worst}");
        assert!(steps.iter().all(|s| *s >= 0.0));
        // And it tracks the engine: never more than ~one block apart.
        let t = 239.0 / 60.0;
        assert!((prev - raw_at(t)).abs() < 2.0 * block * tempo / 60.0 + 0.01);
    }

    #[test]
    fn jumps_snap_and_stop_shows_raw() {
        let t0 = Instant::now();
        let mut c = PlayClock::new(t0);
        c.update(0.0, true, 120.0, None, t0);
        c.update(0.05, true, 120.0, None, t0 + Duration::from_millis(16));
        // Locate far away.
        let v = c.update(32.0, true, 120.0, None, t0 + Duration::from_millis(32));
        assert!((v - 32.0).abs() < 0.2);
        // Stopped: exactly the engine value.
        assert_eq!(c.update(7.25, false, 120.0, None, t0 + Duration::from_millis(48)), 7.25);
    }

    #[test]
    fn loop_wrap_does_not_overshoot() {
        let t0 = Instant::now();
        let mut c = PlayClock::new(t0);
        let lp = Some((0.0, 4.0));
        let mut raw = 3.9;
        c.update(raw, true, 120.0, lp, t0);
        for f in 1..30 {
            let t = t0 + Duration::from_millis(16 * f);
            raw += 0.032;
            if raw >= 4.0 {
                raw -= 4.0;
            }
            let v = c.update(raw, true, 120.0, lp, t);
            assert!(v < 4.0 + 1e-9, "frame {f}: {v}");
        }
    }
}
