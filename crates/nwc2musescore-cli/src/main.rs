//! Command-line driver for `nwc-parse` + `musicxml-write`.

use std::path::PathBuf;

use anyhow::{Context, Result};
use clap::Parser;

#[derive(Debug, Parser)]
#[command(author, version, about, long_about = None)]
struct Args {
    /// Input `.nwc` or `.nwz` file.
    input: PathBuf,

    /// Output `.musicxml` file. Defaults to <input>.musicxml.
    #[arg(short, long)]
    output: Option<PathBuf>,

    /// MusicXML version to emit (3.1 or 4.0). Default: 4.0.
    #[arg(long, default_value = "4.0")]
    musicxml_version: String,

    /// Treat warnings as errors.
    #[arg(long)]
    strict: bool,

    /// Verbose logging.
    #[arg(short, long)]
    verbose: bool,
}

fn main() -> Result<()> {
    let args = Args::parse();
    init_tracing(args.verbose);

    let (score, report) = nwc_parse::parse_file(&args.input)
        .with_context(|| format!("parsing {}", args.input.display()))?;

    for diag in &report.diagnostics {
        match diag.severity {
            nwc_parse::Severity::Error => tracing::error!(offset = diag.offset, "{}", diag.message),
            nwc_parse::Severity::Warn => tracing::warn!(offset = diag.offset, "{}", diag.message),
            nwc_parse::Severity::Info => tracing::info!(offset = diag.offset, "{}", diag.message),
        }
    }

    if args.strict
        && report
            .diagnostics
            .iter()
            .any(|d| d.severity != nwc_parse::Severity::Info)
    {
        anyhow::bail!("--strict: aborting due to {} diagnostic(s)", report.diagnostics.len());
    }

    let opts = musicxml_write::WriteOptions {
        musicxml_version: args.musicxml_version.clone(),
        ..Default::default()
    };
    let xml = musicxml_write::write(&score, &opts)?;

    let out_path = args.output.unwrap_or_else(|| {
        let mut p = args.input.clone();
        p.set_extension("musicxml");
        p
    });

    std::fs::write(&out_path, xml.as_bytes())
        .with_context(|| format!("writing {}", out_path.display()))?;

    tracing::info!(
        objects = report.objects_parsed,
        unknown = report.objects_unknown,
        skipped = report.bytes_skipped,
        "wrote {}",
        out_path.display()
    );
    Ok(())
}

fn init_tracing(verbose: bool) {
    use tracing_subscriber::EnvFilter;
    let filter = if verbose {
        EnvFilter::new("debug")
    } else {
        EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new("info"))
    };
    tracing_subscriber::fmt()
        .with_env_filter(filter)
        .with_target(false)
        .init();
}
