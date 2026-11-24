//! Learn a bank's phoneme inventory from its aliases alone.
//!
//! Vowels: anything that appears as both "- X" (start) and "X -" (end).
//! Consonants: the leftover part of CV tokens ("bU" minus vowel "U" = "b") and
//! the consonant side of VC tokens ("U b", "U b-").

use std::collections::BTreeSet;

use crate::bank::Bank;

#[derive(Debug, Default, Clone)]
pub struct Inventory {
    pub vowels: BTreeSet<String>,
    pub consonants: BTreeSet<String>,
}

impl Inventory {
    pub fn learn(bank: &Bank) -> Self {
        let aliases: Vec<&str> = bank.entries.keys().map(String::as_str).collect();

        let starts: BTreeSet<&str> = aliases.iter().filter_map(|a| a.strip_prefix("- ")).collect();
        let ends: BTreeSet<&str> = aliases.iter().filter_map(|a| a.strip_suffix(" -")).collect();
        let vowels: BTreeSet<String> = starts
            .intersection(&ends)
            .map(|s| s.to_string())
            .collect();

        let mut consonants = BTreeSet::new();
        for a in &aliases {
            for tok in a.split(' ') {
                let tok = tok.trim_end_matches('-'); // "d-" -> "d", "-" -> ""
                if tok.is_empty() || vowels.contains(tok) {
                    continue;
                }
                match split_cv(tok, &vowels) {
                    Some((c, _)) => {
                        consonants.insert(c.to_string());
                    }
                    None => {
                        consonants.insert(tok.to_string());
                    }
                }
            }
        }
        Inventory { vowels, consonants }
    }

    /// (single consonants, clusters, other). "Other" is usually leftovers from
    /// units we can't interpret (e.g. Japanese-style "ba", "ro" in an English bank).
    pub fn consonant_groups(&self) -> (Vec<&str>, Vec<&str>, Vec<&str>) {
        let (mut singles, mut clusters, mut other) = (vec![], vec![], vec![]);
        for c in &self.consonants {
            if crate::phones::SINGLE_CONSONANTS.contains(&c.as_str()) {
                singles.push(c.as_str());
            } else if crate::phones::decomposes(c) {
                clusters.push(c.as_str());
            } else {
                other.push(c.as_str());
            }
        }
        (singles, clusters, other)
    }

    pub fn is_v(&self, s: &str) -> bool {
        self.vowels.contains(s)
    }

    pub fn is_c(&self, s: &str) -> bool {
        self.consonants.contains(s)
    }

    /// Split "bU" into ("b", "U") using the longest matching vowel suffix.
    pub fn split_cv<'a>(&self, tok: &'a str) -> Option<(&'a str, String)> {
        split_cv(tok, &self.vowels).filter(|(c, _)| self.consonants.contains(*c))
    }

    pub fn is_cv(&self, tok: &str) -> bool {
        self.split_cv(tok).is_some()
    }
}

fn split_cv<'a>(tok: &'a str, vowels: &BTreeSet<String>) -> Option<(&'a str, String)> {
    vowels
        .iter()
        .filter(|v| tok.len() > v.len() && tok.ends_with(v.as_str()))
        .max_by_key(|v| v.len())
        .map(|v| (&tok[..tok.len() - v.len()], v.clone()))
}
