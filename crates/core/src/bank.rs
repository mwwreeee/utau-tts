//! Voicebank loading: oto.ini parsing, encoding detection, directory walking.

use std::{
    collections::HashMap,
    fs,
    path::{Path, PathBuf},
};

use anyhow::{bail, Result};
use encoding_rs::SHIFT_JIS;
use walkdir::WalkDir;

/// One line of an oto.ini. All times are in milliseconds.
#[derive(Debug, Clone)]
pub struct OtoEntry {
    pub alias: String,
    pub wav: PathBuf,
    pub offset: f64,
    /// Region (from offset) that must not be time-stretched.
    pub consonant: f64,
    /// Positive: length measured from offset. Negative: trimmed from end of file.
    pub cutoff: f64,
    pub preutter: f64,
    pub overlap: f64,
}

#[derive(Debug, Default)]
pub struct Bank {
    pub name: String,
    pub root: PathBuf,
    /// Keyed by the alias exactly as written. Case matters ("u" != "U", "d" != "D").
    pub entries: HashMap<String, OtoEntry>,
    /// Aliases that appeared more than once (first one wins for now).
    pub duplicates: usize,
    pub bad_lines: usize,
}

/// UTF-8 first (English banks usually are), Shift-JIS as the fallback.
pub fn decode_text(bytes: &[u8]) -> String {
    let bytes = bytes.strip_prefix(b"\xEF\xBB\xBF").unwrap_or(bytes);
    match std::str::from_utf8(bytes) {
        Ok(s) => s.to_owned(),
        Err(_) => SHIFT_JIS.decode(bytes).0.into_owned(),
    }
}

impl Bank {
    pub fn load(root: &Path) -> Result<Self> {
        if !root.is_dir() {
            bail!("{} is not a directory", root.display());
        }
        let mut bank = Bank {
            root: root.to_path_buf(),
            name: root
                .file_name()
                .map(|n| n.to_string_lossy().into_owned())
                .unwrap_or_default(),
            ..Default::default()
        };

        let mut found = 0;
        for e in WalkDir::new(root)
            .follow_links(false)
            .sort_by_file_name()
            .into_iter()
            .filter_map(|e| e.ok())
        {
            if !e.file_type().is_file() {
                continue;
            }
            let lower = e.file_name().to_string_lossy().to_lowercase();
            if lower == "oto.ini" {
                // Each oto.ini is decoded on its own: subfolders can differ in encoding.
                let text = decode_text(&fs::read(e.path())?);
                bank.add_oto_text(&text, e.path().parent().unwrap_or(root));
                found += 1;
            } else if lower == "character.txt" && e.depth() == 1 {
                let text = decode_text(&fs::read(e.path())?);
                if let Some(n) = text
                    .lines()
                    .find_map(|l| l.strip_prefix("name="))
                    .map(|n| n.trim().to_string())
                    .filter(|n| !n.is_empty())
                {
                    bank.name = n;
                }
            }
        }
        if found == 0 {
            bail!("no oto.ini found under {}", root.display());
        }
        Ok(bank)
    }

    /// Parse the text of one oto.ini. `dir` is the folder it lives in; wav paths
    /// are resolved relative to it.
    pub fn add_oto_text(&mut self, text: &str, dir: &Path) {
        for line in text.lines() {
            let Some((wav, rest)) = line.split_once('=') else {
                continue;
            };
            let f: Vec<&str> = rest.split(',').collect();
            if f.len() < 6 {
                self.bad_lines += 1;
                continue;
            }
            // An empty alias means "use the file name without extension".
            let alias = if f[0].is_empty() {
                Path::new(wav)
                    .file_stem()
                    .map(|s| s.to_string_lossy().into_owned())
                    .unwrap_or_default()
            } else {
                f[0].to_string()
            };
            let num = |i: usize| f[i].trim().parse::<f64>().unwrap_or(0.0);
            let entry = OtoEntry {
                alias: alias.clone(),
                wav: dir.join(wav),
                offset: num(1),
                consonant: num(2),
                cutoff: num(3),
                preutter: num(4),
                overlap: num(5),
            };
            if self.entries.contains_key(&alias) {
                self.duplicates += 1;
            } else {
                self.entries.insert(alias, entry);
            }
        }
    }

    pub fn has(&self, alias: &str) -> bool {
        self.entries.contains_key(alias)
    }
}
