pub mod bank;
pub mod inventory;
pub mod phones;
pub mod plan;
pub mod render;
pub mod report;
pub mod text;

use anyhow::Result;

use bank::Bank;
use inventory::Inventory;
use plan::Plan;
use text::Item;

pub enum Planned {
    Units(Plan),
    Pause(f64),
}

pub fn plan_items(bank: &Bank, inv: &Inventory, items: &[Item]) -> (Vec<Planned>, Vec<String>) {
    let mut out = Vec::new();
    let mut warnings = Vec::new();
    for item in items {
        match item {
            Item::Pause(ms) => out.push(Planned::Pause(*ms)),
            Item::Phrase(tokens) => {
                let phones = phones::to_phones(tokens, inv, &mut warnings);
                out.push(Planned::Units(plan::plan(&phones, bank)));
            }
        }
    }
    (out, warnings)
}

pub fn render_planned(bank: &Bank, planned: &[Planned], rate: u32) -> Result<Vec<f32>> {
    render::Renderer::new(bank, rate).render(planned)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::Path;

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
}
