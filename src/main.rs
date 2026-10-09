use std::io::Write;
use std::path::PathBuf;
use std::process::ExitCode;

use clap::{Parser, Subcommand, ValueEnum};
use dtsx_inspect::{inventory, render};

#[derive(Parser)]
#[command(
    name = "dtsx-inspect",
    version,
    about = "Analyse de packages SSIS (.dtsx)"
)]
struct Cli {
    #[command(subcommand)]
    command: Command,
}

#[derive(Subcommand)]
enum Command {
    /// Liste connexions, tâches et composants de Data Flow
    Inventory {
        /// Fichier .dtsx ou dossier (parcouru récursivement)
        path: PathBuf,
        #[arg(long, value_enum, default_value_t = Format::Md)]
        format: Format,
    },
}

#[derive(Clone, Copy, ValueEnum)]
enum Format {
    Md,
    Json,
}

fn main() -> anyhow::Result<ExitCode> {
    let Command::Inventory { path, format } = Cli::parse().command;
    let (inventory, errors) = inventory(&path);
    let output = match format {
        Format::Md => render::markdown(&inventory),
        Format::Json => render::json(&inventory)?,
    };
    std::io::stdout().lock().write_all(output.as_bytes())?;
    for error in &errors {
        eprintln!("erreur : {error}");
    }
    Ok(if errors.is_empty() {
        ExitCode::SUCCESS
    } else {
        ExitCode::FAILURE
    })
}
