use std::io::Write;
use std::path::PathBuf;
use std::process::ExitCode;

use clap::{Parser, Subcommand, ValueEnum};
use dtsx_inspect::lineage::{self, lineage};
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
    /// Lignage en Mermaid : fichiers, tables et entités -> composants et tâches SQL
    Lineage {
        /// Fichier .dtsx ou dossier (parcouru récursivement)
        path: PathBuf,
        /// Seulement l'amont et l'aval des tables, fichiers ou entités dont le
        /// nom contient ce texte (sans casse), par exemple `dbo.Clients`
        #[arg(long)]
        focus: Option<String>,
    },
}

#[derive(Clone, Copy, ValueEnum)]
enum Format {
    Md,
    Json,
}

fn main() -> anyhow::Result<ExitCode> {
    let (output, errors, failure) = match Cli::parse().command {
        Command::Inventory { path, format } => {
            let (inventory, errors) = inventory(&path);
            let output = match format {
                Format::Md => render::markdown(&inventory),
                Format::Json => render::json(&inventory)?,
            };
            (output, errors, None)
        }
        Command::Lineage { path, focus } => {
            let (inventory, errors) = inventory(&path);
            let full = lineage(&inventory);
            match focus
                .as_deref()
                .map(|needle| (needle, lineage::focus(&full, needle)))
            {
                None => (render::mermaid(&full), errors, None),
                Some((_, Some(focused))) => (render::mermaid(&focused), errors, None),
                Some((needle, None)) => {
                    let failure = format!(
                        "aucune table, aucun fichier ni aucune entité ne contient « {needle} »"
                    );
                    (String::new(), errors, Some(failure))
                }
            }
        }
    };
    std::io::stdout().lock().write_all(output.as_bytes())?;
    for error in errors
        .iter()
        .map(ToString::to_string)
        .chain(failure.clone())
    {
        eprintln!("erreur : {error}");
    }
    Ok(if errors.is_empty() && failure.is_none() {
        ExitCode::SUCCESS
    } else {
        ExitCode::FAILURE
    })
}
