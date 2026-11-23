use std::path::PathBuf;

use anyhow::{bail, Result};
use clap::{Parser, Subcommand};
use utau_tts_core::{
    bank::Bank,
    inventory::Inventory,
    plan_items, render_planned, report,
    render::write_wav,
    text::{self, Dict, Item},
    Planned,
};

#[derive(Parser)]
#[command(name = "utau-tts", version, about = "TTS on top of UTAU voicebanks")]
struct Cli {
    #[command(subcommand)]
    cmd: Cmd,
}

#[derive(Subcommand)]
enum Cmd {
    Inspect {
        #[arg(short, long)]
        bank: PathBuf,
        #[arg(long)]
        aliases: bool,
    },
    Say {
        #[arg(short, long)]
        bank: PathBuf,
        #[arg(short, long)]
        text: Option<String>,
        #[arg(long)]
        phones: Option<String>,
        #[arg(short, long)]
        dict: Option<PathBuf>,
        #[arg(short, long, default_value = "out.wav")]
        output: PathBuf,
        #[arg(long, default_value_t = 44100)]
        rate: u32,
        #[arg(long)]
        plan_only: bool,
    },
}

fn main() -> Result<()> {
    match Cli::parse().cmd {
        Cmd::Inspect { bank, aliases } => inspect(bank, aliases),
        Cmd::Say { bank, text, phones, dict, output, rate, plan_only } => {
            say(bank, text, phones, dict, output, rate, plan_only)
        }
    }
}

fn inspect(path: PathBuf, list: bool) -> Result<()> {
    let bank = Bank::load(&path)?;
    let inv = Inventory::learn(&bank);
    let rep = report::build(&bank, &inv);

    println!("bank: {} ({} aliases, {} duplicates, {} bad lines)",
        bank.name, bank.entries.len(), bank.duplicates, bank.bad_lines);
    println!("vowels     ({}): {}", inv.vowels.len(), inv.vowels.iter().cloned().collect::<Vec<_>>().join(" "));
    println!("consonants ({}): {}", inv.consonants.len(), inv.consonants.iter().cloned().collect::<Vec<_>>().join(" "));
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
        let mut all: Vec<_> = bank.entries.keys().collect();
        all.sort();
        println!("\nall aliases:");
        for a in all {
            println!("  {a}");
        }
    }
    Ok(())
}

fn say(
    bank_path: PathBuf,
    text: Option<String>,
    phones: Option<String>,
    dict: Option<PathBuf>,
    output: PathBuf,
    rate: u32,
    plan_only: bool,
) -> Result<()> {
    let bank = Bank::load(&bank_path)?;
    let inv = Inventory::learn(&bank);

    let items: Vec<Item> = match (&phones, &text) {
        (Some(p), _) => vec![Item::Phrase(p.split_whitespace().map(str::to_string).collect())],
        (None, Some(t)) => {
            let Some(d) = dict else { bail!("--text needs --dict <cmudict file>") };
            let dict = Dict::load(&d)?;
            let fe = text::frontend(t, &dict);
            if !fe.oov.is_empty() {
                eprintln!("not in dictionary (dropped): {}", fe.oov.join(", "));
            }
            fe.items
        }
        (None, None) => bail!("give --text or --phones"),
    };

    let (planned, warnings) = plan_items(&bank, &inv, &items);
    for w in &warnings {
        eprintln!("warning: {w}");
    }
    for p in &planned {
        match p {
            Planned::Pause(ms) => println!("[pause {ms} ms]"),
            Planned::Units(plan) => {
                println!("{}", plan.aliases.join(" | "));
                println!("  via {}  (cost {:.1})", plan.trace.join(", "), plan.cost);
                if !plan.skipped.is_empty() {
                    println!("  skipped: {}", plan.skipped.join(" "));
                }
            }
        }
    }
    if plan_only {
        return Ok(());
    }

    let samples = render_planned(&bank, &planned, rate)?;
    write_wav(&output, &samples, rate)?;
    println!("wrote {} ({:.2}s)", output.display(), samples.len() as f32 / rate as f32);
    Ok(())
}
