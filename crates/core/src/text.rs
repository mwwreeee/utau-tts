use std::{collections::HashMap, fs, path::Path};

use anyhow::Result;

pub struct Dict(HashMap<String, Vec<String>>);

impl Dict {
    pub fn load(path: &Path) -> Result<Self> {
        let bytes = fs::read(path)?;
        let decoded: String = if bytes.starts_with(&[0xFF, 0xFE]) {
            encoding_rs::UTF_16LE.decode(&bytes).0.into_owned()
        } else if bytes.starts_with(&[0xFE, 0xFF]) {
            encoding_rs::UTF_16BE.decode(&bytes).0.into_owned()
        } else {
            String::from_utf8_lossy(&bytes).into_owned()
        };
        let text = decoded.trim_start_matches('\u{feff}');
        let mut map = HashMap::new();
        for line in text.lines() {
            if line.starts_with(";;;") || line.starts_with('#') || line.trim().is_empty() {
                continue;
            }
            let mut parts = line.split_whitespace();
            let Some(word) = parts.next() else { continue };
            let word = word.split('(').next().unwrap_or(word).to_lowercase();
            let phones: Vec<String> = parts.take_while(|t| *t != "#").map(str::to_string).collect();
            if phones.is_empty() {
                continue;
            }
            map.entry(word).or_insert(phones);
        }
        Ok(Dict(map))
    }

    pub fn get(&self, word: &str) -> Option<&Vec<String>> {
        self.0.get(word)
    }

    pub fn len(&self) -> usize {
        self.0.len()
    }

    pub fn is_empty(&self) -> bool {
        self.0.is_empty()
    }
}

#[derive(Debug, Clone)]
pub enum Item {
    Phrase(Vec<String>),
    Pause(f64),
}

#[derive(Debug, Default)]
pub struct Frontend {
    pub items: Vec<Item>,
    pub oov: Vec<String>,
}

pub fn frontend(text: &str, dict: &Dict) -> Frontend {
    let mut fe = Frontend::default();
    let mut phrase: Vec<String> = Vec::new();
    let mut word = String::new();

    fn flush_word(word: &mut String, phrase: &mut Vec<String>, fe: &mut Frontend, dict: &Dict) {
        let w = word.trim_matches('\'').to_lowercase();
        word.clear();
        if w.is_empty() {
            return;
        }
        match dict.get(&w) {
            Some(p) => phrase.extend(p.iter().cloned()),
            None => fe.oov.push(w),
        }
    }

    fn flush_phrase(phrase: &mut Vec<String>, fe: &mut Frontend) {
        if !phrase.is_empty() {
            fe.items.push(Item::Phrase(std::mem::take(phrase)));
        }
    }

    for ch in text.chars() {
        if ch.is_alphabetic() || ch == '\'' {
            word.push(ch);
            continue;
        }
        flush_word(&mut word, &mut phrase, &mut fe, dict);
        let pause = match ch {
            ',' => Some(200.0),
            ';' | ':' => Some(250.0),
            '.' | '!' | '?' => Some(450.0),
            '\n' => Some(300.0),
            _ => None,
        };
        if let Some(ms) = pause {
            flush_phrase(&mut phrase, &mut fe);
            fe.items.push(Item::Pause(ms));
        }
    }
    flush_word(&mut word, &mut phrase, &mut fe, dict);
    flush_phrase(&mut phrase, &mut fe);
    fe
}
