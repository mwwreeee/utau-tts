//! Import-time diagnostics: what kinds of units the bank has, what it lacks,
//! and which aliases we couldn't explain (paste these when a new bank misbehaves).

use std::collections::{BTreeMap, BTreeSet};

use crate::{bank::Bank, inventory::Inventory, phones};

#[derive(Debug, Default)]
pub struct Report {
    pub classes: BTreeMap<&'static str, usize>,
    pub unclassified: Vec<String>,
    pub missing_phones: Vec<&'static str>,
    pub missing_wavs: Vec<String>,
}

pub fn classify(alias: &str, inv: &Inventory) -> Option<&'static str> {
    let t: Vec<&str> = alias.split(' ').collect();
    match t.as_slice() {
        ["-", x] => {
            if inv.is_v(x) {
                Some("start_v")
            } else if inv.is_cv(x) {
                Some("start_cv")
            } else if inv.is_c(x) {
                Some("start_c")
            } else {
                None
            }
        }
        [x, "-"] if inv.is_v(x) => Some("v_end"),
        [a, b] if inv.is_v(a) => {
            if inv.is_v(b) {
                Some("vv")
            } else if b.ends_with('-') && inv.is_c(b.trim_end_matches('-')) {
                Some("vc_end")
            } else if inv.is_c(b) {
                Some("vc")
            } else if inv.is_cv(b) {
                Some("vcv")
            } else {
                None
            }
        }
        [a, b] if inv.is_c(a) && b.ends_with('-') && inv.is_c(b.trim_end_matches('-')) => {
            Some("cc_end")
        }
        [a, b] if inv.is_c(a) && inv.is_c(b) => Some("cc"),
        [x] => {
            if inv.is_v(x) {
                Some("bare_v")
            } else if inv.is_cv(x) {
                Some("bare_cv")
            } else if inv.is_c(x) {
                Some("bare_c")
            } else {
                None
            }
        }
        _ => None,
    }
}

pub fn build(bank: &Bank, inv: &Inventory) -> Report {
    let mut r = Report::default();
    let mut aliases: Vec<&String> = bank.entries.keys().collect();
    aliases.sort();
    for a in aliases {
        match classify(a, inv) {
            Some(k) => *r.classes.entry(k).or_insert(0) += 1,
            None => r.unclassified.push(a.clone()),
        }
    }
    r.missing_phones = phones::arpabet_symbols()
        .iter()
        .copied()
        .filter(|a| !phones::resolves(a, inv))
        .collect();
    let missing: BTreeSet<String> = bank
        .entries
        .values()
        .filter(|e| !e.wav.exists())
        .map(|e| e.wav.display().to_string())
        .collect();
    r.missing_wavs = missing.into_iter().collect();
    r
}
