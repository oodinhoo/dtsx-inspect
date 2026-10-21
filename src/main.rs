use std::io::Write;
use std::path::PathBuf;
use std::process::ExitCode;

use anyhow::Context;
use clap::{Parser, Subcommand, ValueEnum};
use dtsx_inspect::calls::calls;
use dtsx_inspect::lineage::columns::column_lineage;
use dtsx_inspect::lineage::order::execution_order;
use dtsx_inspect::lineage::{self, lineage};
use dtsx_inspect::lint::{lint, Severity};
use dtsx_inspect::{inventory, render};

#[derive(Parser)]
#[command(
    name = "dtsx-inspect",
    version,
    about = "Analysis of SSIS packages (.dtsx): inventory, lineage, lint, HTML report"
)]
struct Cli {
    /// Without a command: pick a folder, then the HTML report opens in the browser
    #[command(subcommand)]
    command: Option<Command>,
}

#[derive(Subcommand)]
enum Command {
    /// Lists connections, tasks and Data Flow components
    Inventory {
        /// .dtsx file or folder (walked recursively)
        path: PathBuf,
        #[arg(long, value_enum, default_value_t = Format::Md)]
        format: Format,
    },
    /// Lineage as Mermaid: files, tables and entities -> components and SQL tasks
    Lineage {
        /// .dtsx file or folder (walked recursively)
        path: PathBuf,
        /// Only the upstream and downstream of the tables, files or entities whose
        /// name contains this text (case-insensitive), for instance `dbo.Customers`
        #[arg(long)]
        focus: Option<String>,
    },
    /// SSIS anti-patterns (exit code 1 when an error is found)
    Lint {
        /// .dtsx file or folder (walked recursively)
        path: PathBuf,
    },
    /// Package calls (Execute Package tasks) as Mermaid
    Calls {
        /// Project folder, or parent of several projects
        path: PathBuf,
    },
    /// Lineage of a column in the Data Flows: where it comes from, where it goes
    Columns {
        /// .dtsx file or folder (walked recursively)
        path: PathBuf,
        /// Table or file columns whose name contains this text (case-insensitive),
        /// for instance `dbo.Customers.email`
        #[arg(long)]
        of: String,
    },
    /// Source → target mapping sheet as CSV for Excel (`;`, UTF-8 with BOM)
    Mapping {
        /// .dtsx file or folder (walked recursively)
        path: PathBuf,
        /// CSV file to write; without it, the CSV goes to the console
        #[arg(long)]
        out: Option<PathBuf>,
    },
    /// Execution order inferred from the data: a package after those writing the
    /// tables it reads, in waves, with the cycles and the tables to load first
    Order {
        /// .dtsx file or folder (walked recursively)
        path: PathBuf,
    },
    /// Compares the lineage of two folders, servers aside (exit code 1 when a link or a
    /// package disappeared, or when a link is traced less well)
    Diff {
        /// Folder before the changes
        before: PathBuf,
        /// Folder after the changes
        after: PathBuf,
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
        // Started by double-click: the console closes at once, the error goes through a dialog.
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
                        let failure = format!("no table, file or entity contains \"{needle}\"");
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
                .filter(|f| f.severity == Severity::Error)
                .count();
            let failure = (blocking > 0).then(|| format!("{blocking} lint error(s)"));
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
                    let failure = format!("no table or file column contains \"{of}\"");
                    (String::new(), errors, Some(failure))
                }
            }
        }
        Command::Mapping { path, out } => {
            let (inventory, errors) = inventory(&path);
            let csv = render::mapping_csv(&inventory, &column_lineage(&inventory));
            match out {
                // Console redirected by PowerShell: re-encoded, hence `--out` for Excel.
                Some(file) => {
                    std::fs::write(&file, csv)
                        .with_context(|| format!("writing {}", file.display()))?;
                    (String::new(), errors, None)
                }
                None => (csv, errors, None),
            }
        }
        Command::Order { path } => {
            let (inventory, errors) = inventory(&path);
            let order = execution_order(&lineage(&inventory));
            (render::order_markdown(&inventory, &order), errors, None)
        }
        Command::Diff { before, after } => {
            let (before, mut errors) = inventory(&before);
            let (after, more) = inventory(&after);
            errors.extend(more);
            let (report, regression) = render::mapping_diff(&before, &after);
            let failure =
                regression.then(|| "lineage regression between the two folders".to_owned());
            (report, errors, failure)
        }
    };
    std::io::stdout().lock().write_all(output.as_bytes())?;
    for error in errors
        .iter()
        .map(ToString::to_string)
        .chain(failure.clone())
    {
        eprintln!("error: {error}");
    }
    Ok(if errors.is_empty() && failure.is_none() {
        ExitCode::SUCCESS
    } else {
        ExitCode::FAILURE
    })
}

/// Picks a folder, opens the HTML report (lint, lineage, inventory) in the browser.
fn report() -> anyhow::Result<()> {
    let Some(dir) = rfd::FileDialog::new()
        .set_title("Folder of SSIS packages to analyze")
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
    // shortcut: a single report, overwritten at each run; the browser's "Save as" keeps it.
    let out = std::env::temp_dir().join("dtsx-inspect.html");
    std::fs::write(&out, page)?;
    // `spawn` and not `status`: explorer.exe exits with code 1 even when all is fine.
    std::process::Command::new(OPENER).arg(&out).spawn()?;
    Ok(())
}
