use std::path::PathBuf;

use dtsx_inspect::calls::{calls, Callee};
use dtsx_inspect::{inventory, render};

fn fixture(relative: &str) -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures")
        .join(relative)
}

#[test]
fn execute_package_tasks_point_to_the_called_package() {
    let (inv, errors) = inventory(&fixture("appels"));
    assert!(errors.is_empty(), "{errors:?}");

    let found: Vec<String> = calls(&inv)
        .iter()
        .map(|c| {
            let callee = match &c.callee {
                Callee::Package(i) => inv.packages[*i].name.clone(),
                Callee::Missing(name) => format!("{name} (introuvable)"),
            };
            format!("{} : {} -> {callee}", inv.packages[c.caller].name, c.task)
        })
        .collect();

    // Référence de projet : même dossier ; référence externe : nom du fichier.
    assert_eq!(
        found,
        vec![
            "Charger : PET Fils -> Fils",
            "Orchestrateur : PET Charger -> Charger",
            "Orchestrateur : PET Absent -> 02_Absent.dtsx (introuvable)",
            "Orchestrateur : PET Externe -> Fils",
        ]
    );
}

#[test]
fn mermaid_calls_show_tasks_on_the_arrows_and_missing_packages() {
    let (inv, _) = inventory(&fixture("appels"));

    let chart = render::calls_mermaid(&inv, &calls(&inv));

    assert!(chart.starts_with("flowchart LR\n"), "{chart}");
    assert!(chart.contains("-->|\"PET Charger\"|"), "{chart}");
    assert!(chart.contains("02_Absent.dtsx (introuvable)"), "{chart}");
}
