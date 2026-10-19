use std::io::Write;
use std::path::PathBuf;
use std::process::ExitCode;

use clap::{Parser, Subcommand, ValueEnum};
use dtsx_inspect::calls::calls;
use dtsx_inspect::lineage::columns::column_lineage;
use dtsx_inspect::lineage::{self, lineage};
use dtsx_inspect::lint::{lint, Severity};
use dtsx_inspect::{inventory, render};

#[derive(Parser)]
#[command(
    name = "dtsx-inspect",
    version,
    about = "Analyse de packages SSIS (.dtsx)"
)]
struct Cli {
    /// Sans commande : choix d'un dossier puis rapport HTML dans le navigateur
    #[command(subcommand)]
    command: Option<Command>,
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
    /// Anti-patterns SSIS (code de sortie 1 si une erreur est trouvée)
    Lint {
        /// Fichier .dtsx ou dossier (parcouru récursivement)
        path: PathBuf,
    },
    /// Appels de packages (tâches Execute Package) en Mermaid
    Calls {
        /// Dossier du projet, ou parent de plusieurs projets
        path: PathBuf,
    },
    /// Lignage d'une colonne dans les Data Flows : d'où elle vient, où elle va
    Columns {
        /// Fichier .dtsx ou dossier (parcouru récursivement)
        path: PathBuf,
        /// Colonnes de table ou de fichier dont le nom contient ce texte (sans
        /// casse), par exemple `dbo.Clients.email`
        #[arg(long)]
        of: String,
    },
}

#[derive(Clone, Copy, ValueEnum)]
enum Format {
    Md,
    Json,
}

#[cfg(windows)]
const OPENER: &str = "explorer";
#[cfg(target_os = "macos")]
const OPENER: &str = "open";
#[cfg(not(any(windows, target_os = "macos")))]
const OPENER: &str = "xdg-open";

fn main() -> anyhow::Result<ExitCode> {
    let Some(command) = Cli::parse().command else {
        // Lancé par double-clic : la console se ferme aussitôt, l'erreur passe par une boîte.
        if let Err(error) = report() {
            rfd::MessageDialog::new()
                .set_level(rfd::MessageLevel::Error)
                .set_title("dtsx-inspect")
                .set_description(error.to_string())
                .show();
            return Ok(ExitCode::FAILURE);
        }
        return Ok(ExitCode::SUCCESS);
    };
    let (output, errors, failure) = match command {
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
            match focus {
                None => (render::mermaid(&full), errors, None),
                Some(needle) => match lineage::focus(&full, &needle) {
                    Some(focused) => (render::mermaid(&focused), errors, None),
                    None => {
                        let failure = format!(
                            "aucune table, aucun fichier ni aucune entité ne contient « {needle} »"
                        );
                        (String::new(), errors, Some(failure))
                    }
                },
            }
        }
        Command::Lint { path } => {
            let (inventory, errors) = inventory(&path);
            let findings = lint(&inventory, &lineage(&inventory));
            let blocking = findings
                .iter()
                .filter(|f| f.severity == Severity::Erreur)
                .count();
            let failure = (blocking > 0).then(|| format!("{blocking} erreur(s) de lint"));
            (render::findings(&findings), errors, failure)
        }
        Command::Calls { path } => {
            let (inventory, errors) = inventory(&path);
            let chart = render::calls_mermaid(&inventory, &calls(&inventory));
            (chart, errors, None)
        }
        Command::Columns { path, of } => {
            let (inventory, errors) = inventory(&path);
            match render::column_trace(&column_lineage(&inventory), &of) {
                Some(trace) => (trace, errors, None),
                None => {
                    let failure =
                        format!("aucune colonne de table ni de fichier ne contient « {of} »");
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

/// Choix d'un dossier, rapport HTML (lint, lignage, inventaire) ouvert dans le navigateur.
fn report() -> anyhow::Result<()> {
    let Some(dir) = rfd::FileDialog::new()
        .set_title("Dossier de packages SSIS à analyser")
        .pick_folder()
    else {
        return Ok(());
    };
    let (inventory, errors) = inventory(&dir);
    let lineage = lineage(&inventory);
    let page = render::html(
        &dir.display().to_string(),
        &inventory,
        &lineage,
        &lint(&inventory, &lineage),
        &errors,
    );
    // ponytail: un seul rapport, écrasé à chaque lancement ; « Enregistrer sous » du navigateur pour le garder.
    let out = std::env::temp_dir().join("dtsx-inspect.html");
    std::fs::write(&out, page)?;
    // `spawn` et pas `status` : explorer.exe sort en code 1 même quand tout va bien.
    std::process::Command::new(OPENER).arg(&out).spawn()?;
    Ok(())
}
