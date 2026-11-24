//! Rendering: slice units out of the bank's wavs, apply light per-unit tuning,
//! place them on a timeline using preutterance, and crossfade neighbours over
//! `overlap`.
//!
//! Tuning is deliberately simple: pitch is varispeed resampling (also shifts
//! formants and unit length, so keep it within a few semitones), duration only
//! shortens or lengthens within what the recordings contain, gain is a plain
//! multiplier. Real pitch/time modification (WORLD/PSOLA) comes later.

use std::{
    collections::HashMap,
    f32::consts::FRAC_PI_2,
    path::{Path, PathBuf},
    sync::{Arc, Mutex},
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

/// Linear interpolation reading `data` at `ratio` samples per output sample.
/// ratio > 1 plays faster (higher, shorter); < 1 slower (lower, longer).
fn resample_by_ratio(data: &[f32], ratio: f64) -> Vec<f32> {
    if data.is_empty() || (ratio - 1.0).abs() < 1e-9 || ratio <= 0.0 {
        return data.to_vec();
    }
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

fn resample(data: &[f32], from: u32, to: u32) -> Vec<f32> {
    resample_by_ratio(data, from as f64 / to as f64)
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
    /// Global pitch offset in semitones (varispeed; keep small).
    pub pitch_st: f64,
}

impl Default for RenderOptions {
    fn default() -> Self {
        Self { rate: 44100, speed: 1.0, pitch_st: 0.0 }
    }
}

/// Decoded, resampled wavs shared between renders (GUI re-renders often).
pub type SharedCache = Arc<Mutex<HashMap<(PathBuf, u32), Arc<Vec<f32>>>>>;

pub fn new_cache() -> SharedCache {
    Arc::new(Mutex::new(HashMap::new()))
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
    /// Per-unit multiplier on the note length (1.0 = default).
    pub dur_scale: f64,
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
        let prev_unit = units[u - 1];
        let desired = desired_ms(prev_unit.role, speed) * prev_unit.dur_scale.max(0.05);
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
    cache: SharedCache,
}

impl<'a> Renderer<'a> {
    pub fn new(bank: &'a Bank, opts: RenderOptions) -> Self {
        Self::with_cache(bank, opts, new_cache())
    }

    pub fn with_cache(bank: &'a Bank, opts: RenderOptions, cache: SharedCache) -> Self {
        Self { bank, opts, cache }
    }

    fn ms(&self, x: f64) -> usize {
        (x.max(0.0) * self.opts.rate as f64 / 1000.0).round() as usize
    }

    fn audio(&self, path: &Path) -> Result<Arc<Vec<f32>>> {
        let key = (path.to_path_buf(), self.opts.rate);
        if let Some(a) = self.cache.lock().unwrap().get(&key) {
            return Ok(a.clone());
        }
        let w = read_wav(path).with_context(|| format!("reading {}", path.display()))?;
        let data = Arc::new(resample(&w.data, w.rate, self.opts.rate));
        self.cache.lock().unwrap().insert(key, data.clone());
        Ok(data)
    }

    fn segment(&self, e: &OtoEntry) -> Result<Vec<f32>> {
        let rate = self.opts.rate as f64;
        let to_samples = |ms: f64| (ms * rate / 1000.0).round() as isize;
        let data = self.audio(&e.wav)?;
        let (s, en) = bounds(data.len() as isize, to_samples(e.offset), to_samples(e.cutoff));
        Ok(data[s..en].to_vec())
    }

    fn render_phrase(&self, plan: &Plan) -> Result<Vec<f32>> {
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
            let mut seg = self.segment(e)?;

            let edit = plan.edits.get(k).copied().unwrap_or_default();
            let st = self.opts.pitch_st + edit.pitch_st;
            let ratio = 2f64.powf(st / 12.0);
            if (ratio - 1.0).abs() > 1e-6 {
                seg = resample_by_ratio(&seg, ratio);
            }
            if edit.gain_db.abs() > 1e-6 {
                let g = 10f32.powf(edit.gain_db as f32 / 20.0);
                for s in &mut seg {
                    *s *= g;
                }
            }
            metas.push(UnitMeta {
                len_ms: seg.len() as f64 * 1000.0 / rate,
                // timing fields live on the sample's own time axis, which
                // varispeed scaled
                preutter: e.preutter / ratio,
                overlap: e.overlap / ratio,
                role: plan.roles.get(k).copied().unwrap_or(Role::Cons),
                dur_scale: edit.dur_scale,
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

    pub fn render(&self, planned: &[Planned]) -> Result<Vec<f32>> {
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

    fn meta(len: f64, p: f64, ov: f64, role: Role) -> UnitMeta {
        UnitMeta { len_ms: len, preutter: p, overlap: ov, role, dur_scale: 1.0 }
    }

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
        let cv = meta(443.0, 110.0, 47.0, Role::Vowel(0));
        let vc = meta(583.0, 250.0, 83.0, Role::Cons);
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
        let cv = meta(600.0, 110.0, 47.0, Role::Vowel(1));
        let vc = meta(583.0, 150.0, 83.0, Role::Cons);
        let s = timeline(&[cv, vc], 1.0);
        assert!((s[1].trim - 0.0).abs() < 1e-9);
        assert!((s[1].start - (s[0].t + 190.0 - 150.0)).abs() < 1e-9);
    }

    #[test]
    fn duration_scale_changes_note_length() {
        let mut cv = meta(600.0, 110.0, 47.0, Role::Vowel(1));
        let vc = meta(583.0, 150.0, 83.0, Role::Cons);
        let base = timeline(&[cv, vc], 1.0)[1].t;
        cv.dur_scale = 0.5;
        let short = timeline(&[cv, vc], 1.0)[1].t;
        assert!((base - 110.0 - 190.0).abs() < 1e-9);
        assert!((short - 110.0 - 95.0).abs() < 1e-9);
    }

    #[test]
    fn varispeed_changes_length_by_ratio() {
        let x = vec![0.0f32; 1000];
        assert_eq!(resample_by_ratio(&x, 2.0).len(), 500);
        assert_eq!(resample_by_ratio(&x, 0.5).len(), 2000);
        assert_eq!(resample_by_ratio(&x, 1.0).len(), 1000);
    }
}
