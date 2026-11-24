pub mod bank;
pub mod cv;
pub mod inventory;
pub mod phones;
pub mod plan;
pub mod render;
pub mod report;
pub mod text;
pub mod tuning;

use anyhow::Result;

use bank::Bank;
use inventory::Inventory;
use plan::Plan;
use text::Item;

#[derive(Clone)]
pub enum Planned {
    Units(Plan),
    Pause(f64),
}

/// Which pipeline a bank needs.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Family {
    /// X-SAMPA CVVC with `- X` / `X -` edge units (e.g. Teto English).
    Cvvc,
    /// Plain CV units in romaji or kana (Japanese-style banks).
    Cv,
    /// Aliases are lowercase ARPAbet (`- aa`, `aa -`, `aa r`): ARPAsing style.
    Arpa,
}

impl Family {
    pub fn name(self) -> &'static str {
        match self {
            Family::Cvvc => "X-SAMPA CVVC (cluster-aware)",
            Family::Cv => "pure CV (romaji/kana, katakana-English approximation)",
            Family::Arpa => "ARPAbet phoneme bank (ARPAsing style)",
        }
    }
}

/// ARPAbet-aliased if most lowercase ARPAbet symbols exist as aliases; CVVC if the
/// learned inventory covers most of ARPAbet; otherwise plain CV.
pub fn detect_family(bank: &Bank, inv: &Inventory) -> Family {
    if phones::arpa_coverage(bank).0 >= 25 {
        return Family::Arpa;
    }
    let n = phones::arpabet_symbols()
        .iter()
        .filter(|a| phones::resolves(a, inv))
        .count();
    if n >= 25 {
        Family::Cvvc
    } else {
        Family::Cv
    }
}

/// Text frontend output -> alias plans. Needs no audio, so it works without wavs.
pub fn plan_items(
    bank: &Bank,
    inv: &Inventory,
    family: Family,
    items: &[Item],
) -> (Vec<Planned>, Vec<String>) {
    let cv_index = (family == Family::Cv).then(|| cv::CvIndex::build(bank));
    let mut out = Vec::new();
    let mut warnings = Vec::new();
    for item in items {
        match item {
            Item::Pause(ms) => out.push(Planned::Pause(*ms)),
            Item::Phrase(tokens) => {
                let plan = match (family, &cv_index) {
                    (Family::Cv, Some(idx)) => cv::plan_cv(tokens, idx),
                    (Family::Arpa, _) => plan::plan_arpa(&phones::arpa_phones(tokens, bank), bank),
                    _ => {
                        let ph = phones::to_phones(tokens, inv, &mut warnings);
                        plan::plan(&ph, bank)
                    }
                };
                out.push(Planned::Units(plan));
            }
        }
    }
    (out, warnings)
}

pub fn render_planned(
    bank: &Bank,
    planned: &[Planned],
    opts: &render::RenderOptions,
) -> Result<Vec<f32>> {
    render::Renderer::new(bank, opts.clone()).render(planned)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::Path;

    /// Lines copied from the Teto bank samples, plus a few synthetic ones.
    const SAMPLE: &str = "\
_@_h@_@_@_@-.wav=- @,120.0,124.999,-333.332,0.0,0.0
_@_h@_@_@_@-.wav=@ h@,471.334,375.0,-583.333,250.0,83.333
_@_h@_@_@_@-.wav=h@,611.335,234.998,-443.331,109.998,47.666
_@_h@_@_@_@-.wav=@ @,901.327,375.0,-2174.018,250.0,83.332
_@_h@_@_@_@-.wav=@,901.327,50.0,-2174.018,20.0,40.0
_@_h@_@_@_@-.wav=@ -,3067.347,375.0,-583.333,250.0,83.333
_{_h{_{_{_{-.wav=- {,100.0,109.453,-375.34,0.0,0.0
_{_h{_{_{_{-.wav={ -,3016.231,375.0,-583.333,250.0,83.333
_bu+_bu+_b-.wav=U b,317.668,272.001,-308.668,250.0,83.333
_bu+_bu+_b-.wav=U b-,864.005,506.668,-550.668,250.0,83.333
_b{_b{_b-.wav=- b{,80.0,114.327,-447.661,64.328,33.0
_d{_d{_d-.wav={ d-,919.005,447.999,-492.0,250.0,83.333
";

    fn bank() -> Bank {
        let mut b = Bank::default();
        b.add_oto_text(SAMPLE, Path::new("/bank"));
        b
    }

    #[test]
    fn parses_oto_fields() {
        let b = bank();
        let e = &b.entries["@ h@"];
        assert_eq!(e.offset, 471.334);
        assert_eq!(e.preutter, 250.0);
        assert_eq!(e.overlap, 83.333);
        assert_eq!(b.entries["- @"].cutoff, -333.332);
        assert!(e.wav.ends_with("_@_h@_@_@_@-.wav"));
    }

    #[test]
    fn learns_inventory() {
        let inv = Inventory::learn(&bank());
        assert!(inv.is_v("@") && inv.is_v("{"));
        assert!(inv.is_c("h") && inv.is_c("b") && inv.is_c("d"));
        assert!(!inv.is_v("U")); // never appears as "- U" + "U -" in this sample
    }

    #[test]
    fn plans_cvvc_word() {
        // "bad"
        let b = bank();
        let inv = Inventory::learn(&b);
        let tokens: Vec<String> = ["B", "AE1", "D"].iter().map(|s| s.to_string()).collect();
        let mut w = Vec::new();
        let phones = phones::to_phones(&tokens, &inv, &mut w);
        let p = plan::plan(&phones, &b);
        assert_eq!(p.aliases, vec!["- b{", "{ d-"]);
        assert!(p.skipped.is_empty());
    }

    #[test]
    fn prefers_vcv_when_only_vcv_exists() {
        // schwa, h, schwa
        let b = bank();
        let inv = Inventory::learn(&b);
        let tokens: Vec<String> = ["AH0", "HH", "AH0"].iter().map(|s| s.to_string()).collect();
        let mut w = Vec::new();
        let phones = phones::to_phones(&tokens, &inv, &mut w);
        let p = plan::plan(&phones, &b);
        assert_eq!(p.aliases, vec!["- @", "@ h@", "@ -"]);
    }

    #[test]
    fn missing_units_are_skipped_not_fatal() {
        let b = bank();
        let inv = Inventory::learn(&b);
        let tokens: Vec<String> = ["ZH", "AE1"].iter().map(|s| s.to_string()).collect();
        let mut w = Vec::new();
        let phones = phones::to_phones(&tokens, &inv, &mut w);
        let p = plan::plan(&phones, &b);
        assert!(!p.skipped.is_empty());
    }

    #[test]
    fn classifies_aliases() {
        let b = bank();
        let inv = Inventory::learn(&b);
        assert_eq!(report::classify("- @", &inv), Some("start_v"));
        assert_eq!(report::classify("@ h@", &inv), Some("vcv"));
        assert_eq!(report::classify("{ d-", &inv), Some("vc_end"));
        assert_eq!(report::classify("@", &inv), Some("bare_v"));
    }

    fn oto(aliases: &[&str]) -> String {
        aliases
            .iter()
            .map(|a| format!("x.wav={a},0,0,0,0,0\n"))
            .collect()
    }

    fn plan_for(b: &Bank, arpa: &[&str]) -> plan::Plan {
        let inv = Inventory::learn(b);
        let tokens: Vec<String> = arpa.iter().map(|s| s.to_string()).collect();
        let mut w = Vec::new();
        let phones = phones::to_phones(&tokens, &inv, &mut w);
        plan::plan(&phones, b)
    }

    #[test]
    fn uses_onset_cluster_units() {
        // "street"
        let mut b = Bank::default();
        b.add_oto_text(
            &oto(&["- i", "i -", "- stri", "i t-", "- ri", "- ti", "- si"]),
            Path::new("/bank"),
        );
        let p = plan_for(&b, &["S", "T", "R", "IY1", "T"]);
        assert_eq!(p.aliases, vec!["- stri", "i t-"]);
        assert!(p.skipped.is_empty());
    }

    #[test]
    fn splits_coda_clusters() {
        // "help"
        let mut b = Bank::default();
        b.add_oto_text(&oto(&["- E", "E -", "- hE", "E l", "l p-"]), Path::new("/bank"));
        let p = plan_for(&b, &["HH", "EH1", "L", "P"]);
        assert_eq!(p.aliases, vec!["- hE", "E l", "l p-"]);
        assert!(p.skipped.is_empty());
        let inv = Inventory::learn(&b);
        assert_eq!(report::classify("l p-", &inv), Some("cc_end"));
    }

    #[test]
    fn onset_cluster_unit_then_plain_cv() {
        // Teto convention: "- str" then "ri"
        let mut b = Bank::default();
        b.add_oto_text(&oto(&["- i", "i -", "- str", "ri", "i t-"]), Path::new("/bank"));
        let p = plan_for(&b, &["S", "T", "R", "IY1", "T"]);
        assert_eq!(p.aliases, vec!["- str", "ri", "i t-"]);
        assert!(p.skipped.is_empty());
    }

    #[test]
    fn r_coda_uses_plain_cc_join() {
        // "park": r has "C C" units without a trailing dash
        let mut b = Bank::default();
        b.add_oto_text(&oto(&["- A", "A -", "- pA", "A r", "r k"]), Path::new("/bank"));
        let p = plan_for(&b, &["P", "AA1", "R", "K"]);
        assert_eq!(p.aliases, vec!["- pA", "A r", "r k"]);
        let inv = Inventory::learn(&b);
        assert_eq!(report::classify("r k", &inv), Some("cc"));
    }

    #[test]
    fn arpabet_bank_is_detected_and_planned() {
        let mut aliases: Vec<String> = Vec::new();
        for s in phones::arpabet_symbols() {
            let l = s.to_lowercase();
            aliases.push(format!("- {l}"));
            aliases.push(format!("{l} -"));
            aliases.push(l);
        }
        aliases.extend(["ax", "- ax", "ax l"].iter().map(|s| s.to_string()));
        let refs: Vec<&str> = aliases.iter().map(String::as_str).collect();
        let mut b = Bank::default();
        b.add_oto_text(&oto(&refs), Path::new("/bank"));
        let inv = Inventory::learn(&b);
        assert_eq!(detect_family(&b, &inv), Family::Arpa);

        let tokens: Vec<String> = ["HH", "AH0", "L", "OW1"].iter().map(|s| s.to_string()).collect();
        let p = plan::plan_arpa(&phones::arpa_phones(&tokens, &b), &b);
        assert_eq!(p.aliases, vec!["- hh", "ax", "ax l", "ow -"]);
    }
}
