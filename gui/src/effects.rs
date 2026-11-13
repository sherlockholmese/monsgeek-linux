//! Simulated onboard lighting, for previewing an effect before it is applied.
//!
//! The firmware's exact timing and direction mapping are not documented, so
//! each effect is an approximation of its shape, not a recording of it.
use std::{
    f32::consts::TAU,
    time::{Duration, Instant},
};

use gpui_kit::Hsla;
use monsgeek_linux::keys::{Cap, LAYOUT, LAYOUT_HEIGHT as H, LAYOUT_WIDTH as W};

use crate::app::{Light, hsla};

/// Preview time. Paused clocks hold their frame.
pub struct Clock {
    since: Instant,
    held: Option<f32>,
}

/// A frame that shows every moving effect mid-motion.
const STILL: f32 = 1.3;

impl Clock {
    pub fn new(playing: bool) -> Self {
        let mut clock = Self {
            since: Instant::now(),
            held: Some(STILL),
        };
        if playing {
            clock.toggle();
        }
        clock
    }

    pub fn playing(&self) -> bool {
        self.held.is_none()
    }

    pub fn now(&self) -> f32 {
        self.held
            .unwrap_or_else(|| self.since.elapsed().as_secs_f32())
    }

    pub fn toggle(&mut self) {
        match self.held.take() {
            Some(t) => self.since = Instant::now() - Duration::from_secs_f32(t),
            None => self.held = Some(self.since.elapsed().as_secs_f32()),
        }
    }
}

/// Whether the effect changes over time at these settings.
pub fn moves(light: &Light) -> bool {
    light.brightness > 0 && !matches!(light.mode, 0 | 1 | 13)
}

fn fract(v: f32) -> f32 {
    v.rem_euclid(1.)
}

/// Deterministic noise in 0..1.
fn noise(n: u32) -> f32 {
    let mut x = n.wrapping_add(0x9E37_79B9).wrapping_mul(0x85EB_CA6B);
    x ^= x >> 15;
    x = x.wrapping_mul(0xC2B2_AE35);
    x ^= x >> 13;
    (x & 0xFFFF) as f32 / 65535.
}

fn bump(distance: f32, width: f32) -> f32 {
    (-(distance / width).powi(2)).exp()
}

/// Simulated typing: the latest `count` presses, one every `every` seconds,
/// as (age in seconds, key, seed).
fn taps(t: f32, every: f32, count: u32) -> impl Iterator<Item = (f32, &'static Cap, u32)> {
    let latest = (t / every).floor() as u32;
    (0..count).filter_map(move |back| {
        let n = latest.checked_sub(back)?;
        let cap = &LAYOUT[(noise(n) * LAYOUT.len() as f32) as usize % LAYOUT.len()];
        Some((t - n as f32 * every, cap, n))
    })
}

/// Row index of a key, top to bottom.
fn row(cap: &Cap) -> u32 {
    cap.y as u32
}

/// LED color of one key at time `t`; alpha carries intensity. `None` is dark.
pub fn led(light: &Light, t: f32, cap: &Cap) -> Option<Hsla> {
    if light.brightness == 0 || matches!(light.mode, 0 | 13) {
        return None;
    }
    let p = t * (0.5 + 0.375 * f32::from(light.speed.min(4)));
    let x = cap.x + cap.w / 2.;
    let y = cap.y + 0.5;
    let (nx, ny) = (x / W, y / H);
    let slot = u32::from(cap.slot);
    let reverse = if light.option % 2 == 1 { -1. } else { 1. };
    // A hue-or-intensity sweep: rainbow shows the phase as hue, a single
    // color shows it as a moving crest.
    let sweep = |phase: f32| {
        let phase = fract(phase);
        let level = if light.rainbow {
            1.
        } else {
            0.12 + 0.88 * (0.5 + 0.5 * (TAU * phase).cos())
        };
        (level, phase)
    };
    // (intensity, rainbow hue, hue applies even without rainbow)
    let (level, hue, always_hue) = match light.mode {
        1 => (1., nx * 0.83, false),
        2 => {
            let beat = p * 1.6;
            (
                (0.5 - 0.5 * beat.cos()).powf(1.5),
                fract((beat / TAU).floor() * 0.17),
                false,
            )
        }
        3 => (1., fract(p * 0.12), true),
        4 => {
            let (along, direction) = match light.option {
                0 => (nx, 1.),
                1 => (nx, -1.),
                2 => (ny, 1.),
                _ => (ny, -1.),
            };
            let (level, hue) = sweep(along - direction * p * 0.35);
            (level, hue, false)
        }
        5 => {
            let mut best = (0f32, 0.);
            for (age, origin, seed) in taps(t, 0.8, 4) {
                let d = (x - origin.x - origin.w / 2.).hypot(y - origin.y - 0.5);
                let ring = bump(d - age * 5.5, 0.7) * (1. - age / 3.).max(0.);
                if ring > best.0 {
                    best = (ring, noise(seed + 7));
                }
            }
            (best.0, best.1, false)
        }
        6 => {
            let beat = p * 1.1 + noise(slot) * 7.;
            let n = beat.floor() as u32;
            let on = noise(slot * 31 + n) < 0.14;
            (
                if on { 1. - fract(beat) } else { 0. },
                noise(slot + n * 13),
                false,
            )
        }
        7 => {
            let r = row(cap);
            let across = if r.is_multiple_of(2) { nx } else { 1. - nx };
            let at = (r as f32 + across) / 6.;
            let behind = fract(reverse * (p * 0.14 - at));
            ((1. - behind / 0.16).max(0.), at, false)
        }
        8 => {
            let mut best = (0f32, 0.);
            for (age, origin, seed) in taps(t, 0.3, 5) {
                let level = (1. - age * 0.9).max(0.);
                if origin.slot == cap.slot && level > best.0 {
                    best = (level, noise(seed + 7));
                }
            }
            (best.0, best.1, false)
        }
        9 => {
            let beat = p * 0.3;
            let n = beat.floor() as i32 - i32::from(nx > fract(beat));
            let level = if light.rainbow || n.rem_euclid(2) == 0 {
                1.
            } else {
                0.2
            };
            (level, fract(n as f32 * 0.21), false)
        }
        10 => {
            let crest = H / 2. + (H / 2. - 0.6) * (TAU * (nx * 1.5 - p * 0.3)).sin();
            (bump(y - crest, 0.75), nx * 0.83, false)
        }
        11 => {
            let d = (x - W / 2.).hypot((y - H / 2.) * 1.6) / (W / 2.);
            let (level, hue) = sweep(d * 1.2 - reverse * p * 0.3);
            (level, hue, false)
        }
        12 => {
            let swing = 1. - (fract(p * 0.2) * 2. - 1.).abs();
            let level = if light.option == 0 {
                bump(x - swing * W, 0.9)
            } else {
                bump(y - swing * H, 0.6)
            };
            (level, fract(p * 0.1), false)
        }
        14 => {
            let mut best = (0f32, 0.);
            for (age, origin, seed) in taps(t, 0.7, 3) {
                let reach = (x - origin.x - origin.w / 2.).abs();
                let level = (1. - age * 0.9).max(0.);
                if row(origin) == row(cap) && reach < age * 16. && level > best.0 {
                    best = (level, noise(seed + 7));
                }
            }
            (best.0, best.1, false)
        }
        15 => {
            let angle = (y - H / 2.).atan2(x - W / 2.) / TAU;
            let (level, hue) = sweep(angle - reverse * p * 0.25);
            (level, hue, false)
        }
        16 => (
            1.,
            fract(noise(slot) + p * 0.35 * (0.5 + noise(slot + 99))),
            true,
        ),
        17 => {
            let column = x as u32;
            let fall = p * 0.3 * (0.6 + 0.8 * noise(column)) + noise(column + 50);
            let behind = fract(fall) * (H + 3.) - y;
            let level = if (0. ..2.2).contains(&behind) {
                1. - behind / 2.2
            } else {
                0.
            };
            (level, noise(column + fall.floor() as u32 * 7), false)
        }
        18 => {
            let r = row(cap);
            let flight = p * 0.28 * (0.7 + 0.6 * noise(r)) + noise(r + 20);
            let behind = fract(flight) * (W + 6.) - x;
            let level = if (0. ..5.).contains(&behind) {
                (1. - behind / 5.).powi(2)
            } else {
                0.
            };
            (level, noise(r + flight.floor() as u32 * 7), false)
        }
        19 => {
            let mut level = 1f32;
            for (age, origin, _) in taps(t, 0.3, 5) {
                if origin.slot == cap.slot {
                    level = level.min((age * 0.9).min(1.));
                }
            }
            (level, nx * 0.83, false)
        }
        // Effects this build cannot name are not simulated.
        _ => return None,
    };
    let level = level.clamp(0., 1.) * f32::from(light.brightness.min(4)) / 4.;
    if level < 0.02 {
        return None;
    }
    let color = if light.rainbow || always_hue {
        gpui_kit::hsla(fract(hue), 0.9, 0.55, 1.)
    } else {
        hsla(light.rgb)
    };
    Some(color.opacity(level))
}

#[cfg(test)]
mod tests {
    use super::*;
    use monsgeek_linux::keys::EFFECTS;

    fn light(mode: u8) -> Light {
        Light {
            mode,
            speed: 2,
            brightness: 4,
            option: 0,
            rainbow: false,
            rgb: [0, 200, 255],
        }
    }

    #[test]
    fn every_named_effect_lights_the_board_except_off_and_custom() {
        for (mode, name) in EFFECTS {
            for rainbow in [false, true] {
                for option in 0..monsgeek_linux::keys::effect_options(mode) {
                    let l = Light {
                        rainbow,
                        option,
                        ..light(mode)
                    };
                    let mut lit = 0;
                    for step in 0..200 {
                        for cap in LAYOUT.iter() {
                            if let Some(c) = led(&l, step as f32 * 0.05, cap) {
                                assert!((0. ..=1.).contains(&c.a), "{name}: alpha {}", c.a);
                                assert!(c.h.is_finite() && c.l.is_finite(), "{name}");
                                lit += 1;
                            }
                        }
                    }
                    assert_eq!(lit > 0, !matches!(mode, 0 | 13), "{name}");
                }
            }
        }
    }

    #[test]
    fn brightness_scales_and_zero_is_dark() {
        let cap = &LAYOUT[0];
        let full = led(&light(1), 0., cap).unwrap();
        assert_eq!(full.a, 1.);
        let dim = Light {
            brightness: 2,
            ..light(1)
        };
        assert_eq!(led(&dim, 0., cap).unwrap().a, 0.5);
        let dark = Light {
            brightness: 0,
            ..light(1)
        };
        assert!(led(&dark, 0., cap).is_none());
        assert!(!moves(&dark) && !moves(&light(1)) && moves(&light(4)));
    }

    #[test]
    fn paused_clock_holds_its_frame() {
        let mut clock = Clock::new(false);
        assert!(!clock.playing());
        assert_eq!(clock.now(), STILL);
        clock.toggle();
        assert!(clock.playing() && clock.now() >= STILL);
        clock.toggle();
        let held = clock.now();
        assert_eq!(clock.now(), held);
    }
}
