//! ARPAbet (what CMUdict gives us) -> the bank's X-SAMPA-style symbols.

use crate::{bank::Bank, inventory::Inventory};

#[derive(Debug, Clone)]
pub struct Phone {
    /// Symbol as the *bank* spells it, e.g. "{", "U", "r\\".
    pub sym: String,
    pub vowel: bool,
    /// 0 = unstressed, 1 = primary, 2 = secondary. Unused until prosody lands.
    pub stress: u8,
}

struct Mapping {
    vowel: bool,
    /// Candidate symbols in preference order; the first one the bank has wins.
    cands: &'static [&'static str],
    /// If no candidate exists (diphthongs), emit this sequence of vowels instead.
    split: &'static [&'static str],
}

/// Single X-SAMPA consonants we expect; anything built from these is a cluster.
pub const SINGLE_CONSONANTS: &[&str] = &[
    "tS", "dZ", "b", "d", "D", "f", "g", "h", "k", "l", "m", "n", "N", "p", "r\\", "r", "s", "S",
    "t", "T", "v", "w", "j", "z", "Z",
];

/// True if `s` is a concatenation of single consonants ("str", "kt", "lz").
pub fn decomposes(s: &str) -> bool {
    s.is_empty()
        || SINGLE_CONSONANTS
            .iter()
            .any(|c| s.starts_with(*c) && decomposes(&s[c.len()..]))
}

pub fn arpabet_symbols() -> &'static [&'static str] {
    &[
        "AA", "AE", "AH", "AO", "AW", "AY", "EH", "ER", "EY", "IH", "IY", "OW", "OY", "UH", "UW",
        "B", "CH", "D", "DH", "F", "G", "HH", "JH", "K", "L", "M", "N", "NG", "P", "R", "S", "SH",
        "T", "TH", "V", "W", "Y", "Z", "ZH",
    ]
}

fn mapping(a: &str, stress: u8) -> Option<Mapping> {
    let v = |cands: &'static [&'static str], split: &'static [&'static str]| {
        Some(Mapping { vowel: true, cands, split })
    };
    let c = |cands: &'static [&'static str]| {
        Some(Mapping { vowel: false, cands, split: &[] })
    };
    match a {
        "AA" => v(&["A"], &[]),
        "AE" => v(&["{"], &[]),
        "AH" => {
            if stress == 0 {
                v(&["@", "V"], &[])
            } else {
                v(&["V", "@"], &[])
            }
        }
        "AO" => v(&["O", "A"], &[]),
        "AW" => v(&["aU"], &["A", "U"]),
        "AY" => v(&["aI"], &["A", "I"]),
        "EH" => v(&["E"], &[]),
        "ER" => v(&["3", "@"], &[]),
        "EY" => v(&["eI"], &["E", "I"]),
        "IH" => v(&["I"], &[]),
        "IY" => v(&["i"], &[]),
        "OW" => v(&["oU"], &["O", "U"]),
        "OY" => v(&["OI"], &["O", "I"]),
        "UH" => v(&["U"], &[]),
        "UW" => v(&["u"], &[]),
        "B" => c(&["b"]),
        "CH" => c(&["tS"]),
        "D" => c(&["d"]),
        "DH" => c(&["D"]),
        "F" => c(&["f"]),
        "G" => c(&["g"]),
        "HH" => c(&["h"]),
        "JH" => c(&["dZ"]),
        "K" => c(&["k"]),
        "L" => c(&["l"]),
        "M" => c(&["m"]),
        "N" => c(&["n"]),
        "NG" => c(&["N"]),
        "P" => c(&["p"]),
        "R" => c(&["r\\", "r"]),
        "S" => c(&["s"]),
        "SH" => c(&["S"]),
        "T" => c(&["t"]),
        "TH" => c(&["T"]),
        "V" => c(&["v"]),
        "W" => c(&["w"]),
        "Y" => c(&["j"]),
        "Z" => c(&["z"]),
        "ZH" => c(&["Z"]),
        _ => None,
    }
}

/// "AH0" -> ("AH", 0). A vowel with no digit is treated as stressed.
pub fn split_stress(tok: &str) -> (String, u8) {
    let tok = tok.trim().to_uppercase();
    match tok.chars().last() {
        Some(d) if d.is_ascii_digit() => (
            tok[..tok.len() - 1].to_string(),
            d.to_digit(10).unwrap_or(1) as u8,
        ),
        _ => (tok, 1),
    }
}

/// Can the bank produce this ARPAbet phoneme at all?
pub fn resolves(arpa: &str, inv: &Inventory) -> bool {
    let Some(m) = mapping(arpa, 1) else { return false };
    let set = if m.vowel { &inv.vowels } else { &inv.consonants };
    m.cands.iter().any(|c| set.contains(*c))
        || (!m.split.is_empty() && m.split.iter().all(|c| inv.vowels.contains(*c)))
}

/// Convert ARPAbet tokens ("HH", "AH0", ...) to bank phones.
pub fn to_phones(tokens: &[String], inv: &Inventory, warnings: &mut Vec<String>) -> Vec<Phone> {
    let mut out = Vec::new();
    for tok in tokens {
        let (arpa, stress) = split_stress(tok);
        let Some(m) = mapping(&arpa, stress) else {
            warnings.push(format!("unknown ARPAbet symbol '{tok}', ignored"));
            continue;
        };
        let set = if m.vowel { &inv.vowels } else { &inv.consonants };
        if let Some(sym) = m.cands.iter().copied().find(|c| set.contains(*c)) {
            out.push(Phone { sym: sym.to_string(), vowel: m.vowel, stress });
        } else if !m.split.is_empty() && m.split.iter().all(|c| inv.vowels.contains(*c)) {
            for sym in m.split {
                out.push(Phone { sym: sym.to_string(), vowel: true, stress });
            }
        } else {
            // Keep it so the planner records it as skipped rather than silently losing it.
            out.push(Phone { sym: m.cands[0].to_string(), vowel: m.vowel, stress });
        }
    }
    out
}

// ------------------------------------------------------------------
// ARPAbet-aliased banks (ARPAsing style): aliases are lowercase ARPAbet.

fn is_arpa_vowel(a: &str) -> bool {
    matches!(
        a,
        "AA" | "AE" | "AH" | "AO" | "AW" | "AY" | "EH" | "ER" | "EY" | "IH" | "IY" | "OW" | "OY"
            | "UH" | "UW"
    )
}

/// Does the bank have any unit for this lowercase phoneme?
pub fn arpa_avail(bank: &Bank, sym: &str) -> bool {
    bank.has(sym) || bank.has(&format!("- {sym}")) || bank.has(&format!("{sym} -"))
}

/// (how many of the 39 ARPAbet phonemes the bank has, the ones it lacks)
pub fn arpa_coverage(bank: &Bank) -> (usize, Vec<&'static str>) {
    let mut missing = Vec::new();
    let mut n = 0;
    for a in arpabet_symbols() {
        if arpa_avail(bank, &a.to_lowercase()) {
            n += 1;
        } else {
            missing.push(*a);
        }
    }
    (n, missing)
}

/// Stand-ins used only when the bank lacks the exact phoneme.
fn arpa_fallbacks(sym: &str) -> &'static [&'static str] {
    match sym {
        "er" => &["ah", "ax"],
        "ah" => &["ax", "aa"],
        "ax" => &["ah"],
        "zh" => &["sh", "jh"],
        "uh" => &["uw"],
        "uw" => &["uh"],
        "aa" => &["ao", "ah"],
        "ao" => &["aa", "ow"],
        "ae" => &["eh"],
        "eh" => &["ae", "ih"],
        "ih" => &["iy"],
        "iy" => &["ih"],
        "ow" => &["ao"],
        "oy" => &["ow"],
        "ay" => &["aa"],
        "aw" => &["aa"],
        "ey" => &["eh"],
        "dh" => &["d", "th"],
        "th" => &["s"],
        "jh" => &["ch"],
        "ch" => &["sh"],
        _ => &[],
    }
}

/// ARPAbet tokens -> phones whose `sym` is the bank's lowercase alias token.
pub fn arpa_phones(tokens: &[String], bank: &Bank) -> Vec<Phone> {
    let mut out = Vec::new();
    for tok in tokens {
        let (a, stress) = split_stress(tok);
        if !arpabet_symbols().contains(&a.as_str()) {
            continue;
        }
        let mut sym = a.to_lowercase();
        // Unstressed AH is a schwa; use "ax" when the bank has one.
        if a == "AH" && stress == 0 && arpa_avail(bank, "ax") {
            sym = "ax".to_string();
        }
        if !arpa_avail(bank, &sym) {
            if let Some(f) = arpa_fallbacks(&sym).iter().find(|f| arpa_avail(bank, f)) {
                sym = f.to_string();
            }
        }
        out.push(Phone { sym, vowel: is_arpa_vowel(&a), stress });
    }
    out
}
