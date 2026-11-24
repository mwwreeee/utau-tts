//! Rendering: slice units out of the bank's wavs, place them on a timeline
//! using preutterance, and crossfade neighbours over `overlap`.
//!
//! No pitch or duration modification yet: vowel lengths are capped by what the
//! recordings contain.

use std::{
    collections::HashMap,
    f32::consts::FRAC_PI_2,
    path::{Path, PathBuf},
};

use anyhow::{Context, Result};

use crate::{
    bank::{Bank, OtoEntry},
    plan::{Plan, Role},
    Planned,
};

pub struct Wav {
    pub rate: u32,
    pub data: Vec<f32>,
}

pub fn read_wav(path: &Path) -> Result<Wav> {
    let mut r = hound::WavReader::open(path)?;
    let spec = r.spec();
    let raw: Vec<f32> = match spec.sample_format {
        hound::SampleFormat::Float => r.samples::<f32>().collect::<Result<_, _>>()?,
        hound::SampleFormat::Int => {
            let scale = 1.0 / (1i64 << (spec.bits_per_sample - 1)) as f32;
            r.samples::<i32>()
                .map(|s| s.map(|v| v as f32 * scale))
                .collect::<Result<_, _>>()?
        }
    };
    let ch = spec.channels.max(1) as usize;
    let data = if ch == 1 {
        raw
    } else {
        raw.chunks(ch).map(|f| f.iter().sum::<f32>() / ch as f32).collect()
    };
    Ok(Wav { rate: spec.sample_rate, data })
}

pub fn write_wav(path: &Path, samples: &[f32], rate: u32) -> Result<()> {
    let spec = hound::WavSpec {
        channels: 1,
        sample_rate: rate,
        bits_per_sample: 16,
        sample_format: hound::SampleFormat::Int,
    };
    let mut w = hound::WavWriter::create(path, spec)?;
    for &s in samples {
        w.write_sample((s.clamp(-1.0, 1.0) * i16::MAX as f32) as i16)?;
    }
    w.finalize()?;
    Ok(())
}

/// Linear interpolation. Good enough to get started; swap for `rubato` later.
fn resample(data: &[f32], from: u32, to: u32) -> Vec<f32> {
    if from == to || data.is_empty() {
        return data.to_vec();
    }
    let ratio = from as f64 / to as f64;
    let n = (data.len() as f64 / ratio) as usize;
    (0..n)
        .map(|i| {
            let p = i as f64 * ratio;
            let i0 = p.floor() as usize;
            let f = (p - i0 as f64) as f32;
            let a = data[i0.min(data.len() - 1)];
            let b = data[(i0 + 1).min(data.len() - 1)];
            a + (b - a) * f
        })
        .collect()
}

/// UTAU cutoff convention (verified against real oto.ini data):
/// negative = length of the segment, measured from the offset;
/// positive = amount trimmed from the end of the file; 0 = to the end.
/// All arguments in samples. Returns (start, end).
fn bounds(total: isize, offset: isize, cutoff: isize) -> (usize, usize) {
    let start = offset.clamp(0, total);
    let end = if cutoff < 0 {
        start - cutoff
    } else if cutoff > 0 {
        total - cutoff
    } else {
        total
    };
    (start as usize, end.clamp(start, total) as usize)
}

#[derive(Debug, Clone)]
pub struct RenderOptions {
    pub rate: u32,
    /// 1.0 = default pacing, 2.0 = twice as fast.
    pub speed: f64,
}

impl Default for RenderOptions {
    fn default() -> Self {
        Self { rate: 44100, speed: 1.0 }
    }
}

const MIN_STEP_MS: f64 = 25.0;
const TAIL_FADE_MS: f64 = 10.0;

/// How long a unit's "note" lasts, i.e. time from its onset to the next unit's.
fn desired_ms(role: Role, speed: f64) -> f64 {
    let base = match role {
        Role::Vowel(1) => 190.0,
        Role::Vowel(2) => 150.0,
        Role::Vowel(_) => 100.0,
        Role::Cons | Role::Final => 95.0,
    };
    base / speed.max(0.1)
}

#[derive(Debug, Clone, Copy)]
pub struct UnitMeta {
    pub len_ms: f64,
    pub preutter: f64,
    pub overlap: f64,
    pub role: Role,
}

#[derive(Debug, Clone, Copy)]
pub struct Slot {
    /// Where the unit's sample begins on the phrase timeline (after head trimming).
    pub start: f64,
    /// Milliseconds cut from the head of the sample so it fits.
    pub trim: f64,
    /// Crossfade length with the previous unit.
    pub ov: f64,
    /// Onset: the moment the unit's preutterance point lands.
    pub t: f64,
    /// Sample time remaining after the preutterance point.
    pub rem: f64,
}

/// Place units so each one's preutterance point lands on its onset. A VC unit
/// carries a long vowel head (preutter ~250 ms); when the vowel before it is
/// shorter than that, the head is trimmed instead of doubling the vowel.
pub fn timeline(units: &[UnitMeta], speed: f64) -> Vec<Slot> {
    let mut slots: Vec<Slot> = Vec::with_capacity(units.len());
    for (u, m) in units.iter().enumerate() {
        let p = m.preutter.clamp(0.0, m.len_ms);
        let ov_raw = m.overlap.clamp(0.0, p);
        if u == 0 {
            slots.push(Slot { t: p, start: 0.0, trim: 0.0, ov: 0.0, rem: m.len_ms - p });
            continue;
        }
        let prev = slots[u - 1];
        let desired = desired_ms(units[u - 1].role, speed);
        // Don't ask the previous unit for more audio than it has.
        let d = desired.min(prev.rem + p - ov_raw).max(MIN_STEP_MS);
        let p_eff = p.min(d);
        slots.push(Slot {
            t: prev.t + d,
            start: prev.t + d - p_eff,
            trim: p - p_eff,
            ov: ov_raw.min(p_eff),
            rem: m.len_ms - p,
        });
    }
    slots
}

pub struct Renderer<'a> {
    bank: &'a Bank,
    opts: RenderOptions,
    cache: HashMap<PathBuf, Vec<f32>>,
}

impl<'a> Renderer<'a> {
    pub fn new(bank: &'a Bank, opts: RenderOptions) -> Self {
        Self { bank, opts, cache: HashMap::new() }
    }

    fn ms(&self, x: f64) -> usize {
        (x.max(0.0) * self.opts.rate as f64 / 1000.0).round() as usize
    }

    fn audio(&mut self, path: &Path) -> Result<&Vec<f32>> {
        if !self.cache.contains_key(path) {
            let w = read_wav(path).with_context(|| format!("reading {}", path.display()))?;
            self.cache
                .insert(path.to_path_buf(), resample(&w.data, w.rate, self.opts.rate));
        }
        Ok(&self.cache[path])
    }

    fn segment(&mut self, e: &OtoEntry) -> Result<Vec<f32>> {
        let rate = self.opts.rate as f64;
        let to_samples = |ms: f64| (ms * rate / 1000.0).round() as isize;
        let data = self.audio(&e.wav)?;
        let (s, en) = bounds(data.len() as isize, to_samples(e.offset), to_samples(e.cutoff));
        Ok(data[s..en].to_vec())
    }

    fn render_phrase(&mut self, plan: &Plan) -> Result<Vec<f32>> {
        let n = plan.aliases.len();
        if n == 0 {
            return Ok(Vec::new());
        }
        let bank = self.bank;
        let rate = self.opts.rate as f64;

        let mut segs: Vec<Vec<f32>> = Vec::with_capacity(n);
        let mut metas: Vec<UnitMeta> = Vec::with_capacity(n);
        for (k, alias) in plan.aliases.iter().enumerate() {
            let e = bank
                .entries
                .get(alias)
                .with_context(|| format!("alias '{alias}' vanished from bank"))?;
            let seg = self.segment(e)?;
            metas.push(UnitMeta {
                len_ms: seg.len() as f64 * 1000.0 / rate,
                preutter: e.preutter,
                overlap: e.overlap,
                role: plan.roles.get(k).copied().unwrap_or(Role::Cons),
            });
            segs.push(seg);
        }

        let slots = timeline(&metas, self.opts.speed);
        let to_s = |ms: f64| (ms * rate / 1000.0).round().max(0.0) as usize;
        let start_s: Vec<usize> = slots.iter().map(|s| to_s(s.start)).collect();
        let trim_s: Vec<usize> = slots.iter().map(|s| to_s(s.trim)).collect();
        let ov_s: Vec<usize> = slots.iter().map(|s| to_s(s.ov)).collect();

        let mut out: Vec<f32> = Vec::new();
        for u in 0..n {
            let seg = &segs[u];
            let avail = seg.len().saturating_sub(trim_s[u]);
            let mut end_abs = start_s[u] + avail;
            if u + 1 < n {
                // The previous unit ends where the next one's crossfade ends.
                end_abs = end_abs.min(start_s[u + 1] + ov_s[u + 1]);
            }
            let len = end_abs.saturating_sub(start_s[u]);
            if len == 0 {
                continue;
            }
            let body = &seg[trim_s[u]..trim_s[u] + len];
            let fade_in = if u == 0 { 0 } else { ov_s[u].min(len) };
            let fade_out = if u + 1 < n {
                end_abs.saturating_sub(start_s[u + 1]).min(len)
            } else {
                to_s(TAIL_FADE_MS).min(len)
            };
            if out.len() < end_abs {
                out.resize(end_abs, 0.0);
            }
            for (k, &x) in body.iter().enumerate() {
                let mut g = 1.0f32;
                if k < fade_in {
                    let w = (k as f32 + 0.5) / fade_in as f32;
                    g *= (w * FRAC_PI_2).sin().powi(2);
                }
                if k >= len - fade_out {
                    let w = ((k - (len - fade_out)) as f32 + 0.5) / fade_out as f32;
                    g *= (w * FRAC_PI_2).cos().powi(2);
                }
                out[start_s[u] + k] += x * g;
            }
        }
        Ok(out)
    }

    pub fn render(&mut self, planned: &[Planned]) -> Result<Vec<f32>> {
        let mut out: Vec<f32> = Vec::new();
        for p in planned {
            match p {
                Planned::Pause(ms) => {
                    let n = self.ms(*ms / self.opts.speed.max(0.1));
                    out.extend(std::iter::repeat(0.0).take(n));
                }
                Planned::Units(plan) => {
                    let phrase = self.render_phrase(plan)?;
                    out.extend(phrase);
                }
            }
        }
        let peak = out.iter().fold(0.0f32, |m, s| m.max(s.abs()));
        if peak > 0.99 {
            for s in &mut out {
                *s *= 0.99 / peak;
            }
        }
        Ok(out)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cutoff_conventions() {
        assert_eq!(bounds(1000, 100, -300), (100, 400)); // length from offset
        assert_eq!(bounds(1000, 100, 200), (100, 800)); // trimmed from end
        assert_eq!(bounds(1000, 100, 0), (100, 1000));
        assert_eq!(bounds(1000, 900, -300), (900, 1000)); // clamped to file
    }

    #[test]
    fn vc_head_is_trimmed_when_vowel_is_short() {
        // numbers from the Teto bank: "h@" and "@ l"-style VC
        let cv = UnitMeta { len_ms: 443.0, preutter: 110.0, overlap: 47.0, role: Role::Vowel(0) };
        let vc = UnitMeta { len_ms: 583.0, preutter: 250.0, overlap: 83.0, role: Role::Cons };
        let s = timeline(&[cv, vc], 1.0);
        assert!((s[0].start - 0.0).abs() < 1e-9);
        assert!((s[0].t - 110.0).abs() < 1e-9);
        // unstressed vowel lasts 100 ms: the VC starts at the CV's onset and
        // loses the first 150 ms of its 250 ms vowel head
        assert!((s[1].start - 110.0).abs() < 1e-9);
        assert!((s[1].trim - 150.0).abs() < 1e-9);
        assert!((s[1].ov - 83.0).abs() < 1e-9);
    }

    #[test]
    fn long_vowel_needs_no_trim() {
        let cv = UnitMeta { len_ms: 600.0, preutter: 110.0, overlap: 47.0, role: Role::Vowel(1) };
        let vc = UnitMeta { len_ms: 583.0, preutter: 150.0, overlap: 83.0, role: Role::Cons };
        let s = timeline(&[cv, vc], 1.0);
        assert!((s[1].trim - 0.0).abs() < 1e-9);
        assert!((s[1].start - (s[0].t + 190.0 - 150.0)).abs() < 1e-9);
    }
}
