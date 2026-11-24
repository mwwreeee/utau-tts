use std::path::PathBuf;

use anyhow::{bail, Result};
use clap::{Parser, Subcommand, ValueEnum};
use utau_tts_core::{
    bank::Bank,
    cv::CvIndex,
    detect_family,
    inventory::Inventory,
    plan_items, render_planned, report,
    render::{write_wav, RenderOptions},
    text::{self, Dict, Item},
    phones as ph,
    Family, Planned,
};

#[derive(Parser)]
#[command(name = "utau-tts", version, about = "TTS on top of UTAU voicebanks")]
struct Cli {
    #[command(subcommand)]
    cmd: Cmd,
}

#[derive(Clone, Copy, ValueEnum)]
enum FamilyArg {
    /// Detect from the bank's aliases.
    Auto,
    /// X-SAMPA CVVC with edge units (Teto English style).
    Cvvc,
    /// Plain CV units in romaji or kana.
    Cv,
    /// Lowercase ARPAbet aliases (ARPAsing style).
    Arpa,
}

#[derive(Subcommand)]
enum Cmd {
    /// Load a bank and print what we understood about it.
    Inspect {
        #[arg(short, long)]
        bank: PathBuf,
        /// Also list every alias.
        #[arg(long)]
        aliases: bool,
    },
    /// List aliases containing a substring (sorted), with their timing fields.
    Find {
        #[arg(short, long)]
        bank: PathBuf,
        pattern: String,
        #[arg(long, default_value_t = 80)]
        limit: usize,
    },
    /// Synthesize speech.
    Say {
        #[arg(short, long)]
        bank: PathBuf,
        /// Text to speak (needs --dict).
        #[arg(short, long)]
        text: Option<String>,
        /// ARPAbet instead of text, e.g. "HH AH0 L OW1".
        #[arg(long)]
        phones: Option<String>,
        /// CMUdict file (cmudict.dict or cmudict-0.7b).
        #[arg(short, long)]
        dict: Option<PathBuf>,
        #[arg(short, long, default_value = "out.wav")]
        output: PathBuf,
        #[arg(long, default_value_t = 44100)]
        rate: u32,
        /// Pacing: 1.0 default, 1.3 faster, 0.8 slower.
        #[arg(long, default_value_t = 1.0)]
        speed: f64,
        /// Global pitch offset in semitones (varispeed; keep within about +/-3).
        #[arg(long, default_value_t = 0.0, allow_negative_numbers = true)]
        pitch: f64,
        /// Bank family; auto-detected by default.
        #[arg(long, value_enum, default_value_t = FamilyArg::Auto)]
        family: FamilyArg,
        /// Print the alias plan and stop; no audio is read.
        #[arg(long)]
        plan_only: bool,
    },
}

impl Default for FamilyArg {
    fn default() -> Self {
        FamilyArg::Auto
    }
}

fn main() -> Result<()> {
    match Cli::parse().cmd {
        Cmd::Inspect { bank, aliases } => inspect(bank, aliases),
        Cmd::Find { bank, pattern, limit } => find(bank, pattern, limit),
        Cmd::Say { bank, text, phones, dict, output, rate, speed, pitch, family, plan_only } => {
            say(bank, text, phones, dict, output, rate, speed, pitch, family, plan_only)
        }
    }
}

fn pick_family(arg: FamilyArg, bank: &Bank, inv: &Inventory) -> Family {
    match arg {
        FamilyArg::Auto => detect_family(bank, inv),
        FamilyArg::Cvvc => Family::Cvvc,
        FamilyArg::Cv => Family::Cv,
        FamilyArg::Arpa => Family::Arpa,
    }
}

fn inspect(path: PathBuf, list: bool) -> Result<()> {
    let bank = Bank::load(&path)?;
    let inv = Inventory::learn(&bank);
    let family = detect_family(&bank, &inv);

    println!("bank: {} ({} aliases, {} duplicates, {} bad lines)",
        bank.name, bank.entries.len(), bank.duplicates, bank.bad_lines);
    println!("family: {}", family.name());

    if family == Family::Arpa {
        let (n, missing) = ph::arpa_coverage(&bank);
        println!("\nARPAbet phonemes available: {n}/39");
        if !missing.is_empty() {
            println!("missing: {}", missing.join(" "));
        }
        let mut trans: Vec<&String> = bank
            .entries
            .keys()
            .filter(|a| a.contains(' ') && !a.starts_with("- ") && !a.ends_with(" -"))
            .collect();
        trans.sort();
        println!(
            "\ntransition units ({}): {}",
            trans.len(),
            trans.iter().map(|s| s.as_str()).collect::<Vec<_>>().join(" | ")
        );
        if list {
            print_all(&bank);
        }
        return Ok(());
    }

    if family == Family::Cv {
        let idx = CvIndex::build(&bank);
        let missing = idx.missing_basic();
        println!("\n{} aliases map to a kana/romaji key", idx.len());
        if missing.is_empty() {
            println!("all basic kana are present");
        } else {
            println!("basic kana missing ({}): {}", missing.len(), missing.join(" "));
        }
        println!("(English is approximated as katakana-style moras; run `say --plan-only` to see them)");
        if list {
            print_all(&bank);
        }
        return Ok(());
    }

    let rep = report::build(&bank, &inv);
    println!("vowels     ({}): {}", inv.vowels.len(), inv.vowels.iter().cloned().collect::<Vec<_>>().join(" "));
    let (singles, clusters, other) = inv.consonant_groups();
    println!("consonants ({}): {}", singles.len(), singles.join(" "));
    println!("cluster tokens ({}): {}", clusters.len(), clusters.join(" "));
    println!("other tokens ({}, probably unusable): {}", other.len(), other.join(" "));
    println!("\nunit classes:");
    for (k, n) in &rep.classes {
        println!("  {k:<10} {n}");
    }
    if rep.missing_phones.is_empty() {
        println!("\nevery ARPAbet phoneme maps to something in this bank");
    } else {
        println!("\nARPAbet phonemes with no match: {}", rep.missing_phones.join(" "));
    }
    if !rep.missing_wavs.is_empty() {
        println!("\n{} wav files referenced but not found (first 10):", rep.missing_wavs.len());
        for w in rep.missing_wavs.iter().take(10) {
            println!("  {w}");
        }
    }
    if !rep.unclassified.is_empty() {
        println!("\n{} aliases matched no known pattern (first 60):", rep.unclassified.len());
        for a in rep.unclassified.iter().take(60) {
            println!("  {a}");
        }
    }
    if list {
        print_all(&bank);
    }
    Ok(())
}

fn print_all(bank: &Bank) {
    let mut all: Vec<_> = bank.entries.keys().collect();
    all.sort();
    println!("\nall aliases:");
    for a in all {
        println!("  {a}");
    }
}

fn find(path: PathBuf, pattern: String, limit: usize) -> Result<()> {
    let bank = Bank::load(&path)?;
    let mut hits: Vec<_> = bank.entries.values().filter(|e| e.alias.contains(&pattern)).collect();
    hits.sort_by(|a, b| a.alias.cmp(&b.alias));
    println!("{} aliases contain {:?}", hits.len(), pattern);
    for e in hits.iter().take(limit) {
        println!("  {:<12} pre {:>7.1}  ovl {:>6.1}  cons {:>7.1}", e.alias, e.preutter, e.overlap, e.consonant);
    }
    Ok(())
}

#[allow(clippy::too_many_arguments)]
fn say(
    bank_path: PathBuf,
    text_arg: Option<String>,
    phones: Option<String>,
    dict: Option<PathBuf>,
    output: PathBuf,
    rate: u32,
    speed: f64,
    pitch: f64,
    family_arg: FamilyArg,
    plan_only: bool,
) -> Result<()> {
    let bank = Bank::load(&bank_path)?;
    let inv = Inventory::learn(&bank);
    let family = pick_family(family_arg, &bank, &inv);
    eprintln!("bank family: {}", family.name());

    let items: Vec<Item> = match (&phones, &text_arg) {
        (Some(p), _) => vec![Item::Phrase(p.split_whitespace().map(str::to_string).collect())],
        (None, Some(t)) => {
            let Some(d) = dict else { bail!("--text needs --dict <cmudict file>") };
            let dict = Dict::load(&d)?;
            eprintln!("dictionary: {} entries", dict.len());
            if dict.len() < 1000 {
                bail!(
                    "{} has only {} entries, which doesn't look like CMUdict. \
                     Download the raw cmudict.dict file, not the GitHub web page.",
                    d.display(),
                    dict.len()
                );
            }
            let fe = text::frontend(t, &dict);
            if !fe.oov.is_empty() {
                eprintln!("not in dictionary (dropped): {}", fe.oov.join(", "));
            }
            for it in &fe.items {
                if let Item::Phrase(p) = it {
                    eprintln!("phones: {}", p.join(" "));
                }
            }
            fe.items
        }
        (None, None) => bail!("give --text or --phones"),
    };

    let (planned, warnings) = plan_items(&bank, &inv, family, &items);
    for w in &warnings {
        eprintln!("warning: {w}");
    }
    let mut total_units = 0;
    for p in &planned {
        match p {
            Planned::Pause(ms) => println!("[pause {ms} ms]"),
            Planned::Units(plan) => {
                total_units += plan.aliases.len();
                println!("{}", plan.aliases.join(" | "));
                println!("  via {}  (cost {:.1})", plan.trace.join(", "), plan.cost);
                if !plan.skipped.is_empty() {
                    println!("  skipped: {}", plan.skipped.join(" "));
                }
            }
        }
    }
    if total_units == 0 {
        bail!(
            "nothing to say: no units were planned. Either every word was missing from the \
             dictionary (see above) or the bank has none of the units needed."
        );
    }
    if plan_only {
        return Ok(());
    }

    let samples = render_planned(&bank, &planned, &RenderOptions { rate, speed, pitch_st: pitch })?;
    write_wav(&output, &samples, rate)?;
    println!("wrote {} ({:.2}s)", output.display(), samples.len() as f32 / rate as f32);
    Ok(())
}
