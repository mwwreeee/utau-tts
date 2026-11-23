use std::{
    collections::HashMap,
    f32::consts::FRAC_PI_2,
    path::{Path, PathBuf},
};

use anyhow::{Context, Result};

use crate::{bank::{Bank, OtoEntry}, Planned};

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

fn append_with_overlap(out: &mut Vec<f32>, seg: &[f32], overlap: usize) {
    let ov = overlap.min(seg.len()).min(out.len());
    let start = out.len() - ov;
    for i in 0..ov {
        let t = (i as f32 + 0.5) / ov as f32;
        let (fade_in, fade_out) = ((t * FRAC_PI_2).sin(), (t * FRAC_PI_2).cos());
        out[start + i] = out[start + i] * fade_out + seg[i] * fade_in;
    }
    out.extend_from_slice(&seg[ov..]);
}

pub struct Renderer<'a> {
    bank: &'a Bank,
    rate: u32,
    cache: HashMap<PathBuf, Vec<f32>>,
}

impl<'a> Renderer<'a> {
    pub fn new(bank: &'a Bank, rate: u32) -> Self {
        Self { bank, rate, cache: HashMap::new() }
    }

    fn ms(&self, x: f64) -> usize {
        (x.max(0.0) * self.rate as f64 / 1000.0).round() as usize
    }

    fn audio(&mut self, path: &Path) -> Result<&Vec<f32>> {
        if !self.cache.contains_key(path) {
            let w = read_wav(path).with_context(|| format!("reading {}", path.display()))?;
            self.cache
                .insert(path.to_path_buf(), resample(&w.data, w.rate, self.rate));
        }
        Ok(&self.cache[path])
    }

    fn segment(&mut self, e: &OtoEntry) -> Result<Vec<f32>> {
        let rate = self.rate as f64;
        let to_samples = |ms: f64| (ms * rate / 1000.0).round() as isize;
        let data = self.audio(&e.wav)?;
        let total = data.len() as isize;
        let start = to_samples(e.offset).clamp(0, total);
        let raw_end = if e.cutoff < 0.0 {
            total + to_samples(e.cutoff)
        } else if e.cutoff > 0.0 {
            start + to_samples(e.cutoff)
        } else {
            total
        };
        let end = raw_end.clamp(start, total);
        Ok(data[start as usize..end as usize].to_vec())
    }

    pub fn render(&mut self, planned: &[Planned]) -> Result<Vec<f32>> {
        let bank = self.bank;
        let mut out: Vec<f32> = Vec::new();
        for p in planned {
            match p {
                Planned::Pause(ms) => {
                    out.extend(std::iter::repeat(0.0).take(self.ms(*ms)));
                }
                Planned::Units(plan) => {
                    for (k, alias) in plan.aliases.iter().enumerate() {
                        let entry = bank
                            .entries
                            .get(alias)
                            .with_context(|| format!("alias '{alias}' vanished from bank"))?;
                        let seg = self.segment(entry)?;
                        let ov = if k == 0 { 0 } else { self.ms(entry.overlap) };
                        append_with_overlap(&mut out, &seg, ov);
                    }
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
