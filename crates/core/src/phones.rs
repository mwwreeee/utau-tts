use crate::inventory::Inventory;

#[derive(Debug, Clone)]
pub struct Phone {
    pub sym: String,
    pub vowel: bool,
    pub stress: u8,
}

struct Mapping {
    vowel: bool,
    cands: &'static [&'static str],
    split: &'static [&'static str],
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

pub fn resolves(arpa: &str, inv: &Inventory) -> bool {
    let Some(m) = mapping(arpa, 1) else { return false };
    let set = if m.vowel { &inv.vowels } else { &inv.consonants };
    m.cands.iter().any(|c| set.contains(*c))
        || (!m.split.is_empty() && m.split.iter().all(|c| inv.vowels.contains(*c)))
}

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
            out.push(Phone { sym: m.cands[0].to_string(), vowel: m.vowel, stress });
        }
    }
    out
}
