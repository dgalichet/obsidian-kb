use anyhow::{Context, Result, bail};
use obsidian_kb::synthetic::{SyntheticLinkMode, SyntheticVaultOptions, generate_synthetic_vault};
use std::path::PathBuf;

fn main() {
    if let Err(error) = run() {
        eprintln!("error: {error:?}");
        std::process::exit(1);
    }
}

fn run() -> Result<()> {
    let mut output = None;
    let mut chunks = 1_000usize;
    let mut chunks_per_note = 100usize;
    let mut link_mode = SyntheticLinkMode::Sparse;
    let mut overwrite = false;

    let mut args = std::env::args().skip(1);
    while let Some(arg) = args.next() {
        match arg.as_str() {
            "--out" => {
                output = Some(PathBuf::from(
                    args.next().context("missing value after --out")?,
                ));
            }
            "--chunks" => {
                chunks = args
                    .next()
                    .context("missing value after --chunks")?
                    .parse()
                    .context("--chunks must be a positive integer")?;
            }
            "--chunks-per-note" => {
                chunks_per_note = args
                    .next()
                    .context("missing value after --chunks-per-note")?
                    .parse()
                    .context("--chunks-per-note must be a positive integer")?;
            }
            "--links" => {
                link_mode =
                    SyntheticLinkMode::parse(&args.next().context("missing value after --links")?)?;
            }
            "--overwrite" => {
                overwrite = true;
            }
            "--help" | "-h" => {
                print_help();
                return Ok(());
            }
            other => bail!("unknown argument `{other}`"),
        }
    }

    let output = output.context("missing --out /path/to/vault")?;
    let report = generate_synthetic_vault(
        &output,
        &SyntheticVaultOptions {
            chunks,
            chunks_per_note,
            link_mode,
            overwrite,
        },
    )?;
    println!("{}", serde_json::to_string_pretty(&report)?);
    Ok(())
}

fn print_help() {
    println!(
        "Usage: cargo run --example generate_synthetic_vault -- --out /tmp/vault --chunks 10000 --chunks-per-note 100 --links sparse --overwrite"
    );
}
