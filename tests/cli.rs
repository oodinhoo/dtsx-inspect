use std::process::{Command, Output};

fn run(args: &[&str]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_dtsx-inspect"))
        .args(args)
        .current_dir(env!("CARGO_MANIFEST_DIR"))
        .output()
        .expect("running the binary")
}

fn stdout(output: &Output) -> String {
    String::from_utf8_lossy(&output.stdout).into_owned()
}

fn stderr(output: &Output) -> String {
    String::from_utf8_lossy(&output.stderr).into_owned()
}

#[test]
fn inventory_defaults_to_markdown() {
    let out = run(&["inventory", "tests/fixtures/minimal.dtsx"]);

    assert!(out.status.success(), "{}", stderr(&out));
    assert!(
        stdout(&out).starts_with("# Demo_Minimal"),
        "{}",
        stdout(&out)
    );
}

#[test]
fn inventory_json_is_valid_json() {
    let out = run(&["inventory", "tests/fixtures/project", "--format", "json"]);

    assert!(out.status.success(), "{}", stderr(&out));
    let value: serde_json::Value = serde_json::from_slice(&out.stdout).expect("valid JSON");
    assert_eq!(value["packages"][0]["name"], "Chargement");
}

#[test]
fn never_prints_secrets() {
    for format in ["md", "json"] {
        let out = run(&["inventory", "tests/fixtures", "--format", format]);

        assert!(stdout(&out).contains("Demo_Minimal"), "{format}");
        assert!(
            !stdout(&out).contains("s3cr3t"),
            "leak on stdout in {format}"
        );
        assert!(
            !stderr(&out).contains("s3cr3t"),
            "leak on stderr in {format}"
        );
    }
}

#[test]
fn broken_files_fail_the_run_and_are_named() {
    let out = run(&["inventory", "tests/fixtures/broken"]);

    assert_eq!(out.status.code(), Some(1));
    assert!(stdout(&out).contains("# Valide"));
    assert!(stderr(&out).contains("corrompu.dtsx"), "{}", stderr(&out));
    assert!(stderr(&out).contains("ancien.dtsx"), "{}", stderr(&out));
}

#[test]
fn missing_path_fails_the_run() {
    let out = run(&["inventory", "tests/fixtures/absent"]);

    assert_eq!(out.status.code(), Some(1));
}

#[test]
fn lineage_prints_mermaid_without_secrets_or_sql() {
    let out = run(&["lineage", "tests/fixtures/lineage"]);

    assert!(out.status.success(), "{}", stderr(&out));
    let text = stdout(&out);
    assert!(text.starts_with("flowchart LR"), "{text}");
    assert!(!text.contains("s3cr3t"), "secret leak");
    assert!(
        !text.contains("SELECT c.id"),
        "the SQL text must not be displayed"
    );
}

#[test]
fn lineage_focus_shows_only_the_chain_of_a_table() {
    let out = run(&[
        "lineage",
        "tests/fixtures/lineage",
        "--focus",
        "dbo.Historique",
    ]);

    assert!(out.status.success(), "{}", stderr(&out));
    let text = stdout(&out);
    assert!(text.contains("SQL Historiser"), "{text}");
    assert!(!text.contains("CRM Comptes"), "{text}");
}

#[test]
fn lineage_focus_without_match_fails() {
    let out = run(&["lineage", "tests/fixtures/lineage", "--focus", "missing"]);

    assert_eq!(out.status.code(), Some(1));
    assert!(stderr(&out).contains("missing"), "{}", stderr(&out));
}

#[test]
fn lint_fails_on_errors_and_prints_one_line_per_finding() {
    let out = run(&["lint", "tests/fixtures/minimal.dtsx"]);

    assert_eq!(out.status.code(), Some(1));
    let text = stdout(&out);
    assert!(
        text.contains("error[unresolved-connection] minimal.dtsx: SQL Ancienne purge: "),
        "{text}"
    );
    assert!(!text.contains("s3cr3t"), "secret leak");
}

#[test]
fn lint_passes_on_a_clean_package() {
    let out = run(&["lint", "tests/fixtures/broken/valide.dtsx"]);

    assert!(out.status.success(), "{}", stderr(&out));
    assert_eq!(stdout(&out), "0 error(s), 0 warning(s), 0 info(s)\n");
}

#[test]
fn lineage_reports_broken_files() {
    let out = run(&["lineage", "tests/fixtures/broken"]);

    assert_eq!(out.status.code(), Some(1));
    assert!(stderr(&out).contains("corrompu.dtsx"), "{}", stderr(&out));
}

#[test]
fn calls_prints_the_package_call_graph_in_mermaid() {
    let out = run(&["calls", "tests/fixtures/calls"]);

    assert!(out.status.success(), "{}", stderr(&out));
    let chart = stdout(&out);
    assert!(chart.starts_with("flowchart LR\n"), "{chart}");
    assert!(chart.contains("\"Orchestrateur\""), "{chart}");
    assert!(chart.contains("02_Absent.dtsx (not found)"), "{chart}");
}

#[test]
fn columns_traces_a_column_upstream_and_downstream() {
    let out = run(&[
        "columns",
        "tests/fixtures/columns",
        "--of",
        "dbo.Clients.email",
    ]);

    assert!(out.status.success(), "{}", stderr(&out));
    let trace = stdout(&out);
    assert!(trace.starts_with("Entrepot : dbo.Clients.email"), "{trace}");
    assert!(trace.contains("← expression: FF Clients.email"), "{trace}");
    assert!(
        trace.contains("→ Union All: UNI Fusion.courriel"),
        "{trace}"
    );
}

#[test]
fn columns_without_match_fails() {
    let out = run(&["columns", "tests/fixtures/columns", "--of", "missing"]);

    assert!(!out.status.success());
    assert!(stderr(&out).contains("missing"), "{}", stderr(&out));
}

#[test]
fn order_prints_the_waves_deduced_from_the_data() {
    let out = run(&["order", "tests/fixtures/order"]);

    assert!(out.status.success(), "{}", stderr(&out));
    let text = stdout(&out);
    let first = text.find("## Wave 1").expect("wave 1");
    let second = text
        .find("- Charger — after Extraire")
        .expect("Charger after Extraire");
    assert!(first < second, "{text}");
}

#[test]
fn diff_fails_only_when_the_lineage_regresses() {
    let same = run(&[
        "diff",
        "tests/fixtures/columns_sql",
        "tests/fixtures/columns_sql",
    ]);
    let regressed = run(&[
        "diff",
        "tests/fixtures/diff/before",
        "tests/fixtures/diff/after",
    ]);

    assert!(same.status.success(), "{}", stderr(&same));
    assert!(
        stdout(&same).contains("No lineage difference."),
        "{}",
        stdout(&same)
    );
    assert!(!regressed.status.success());
    assert!(
        stdout(&regressed).contains("## Removed links (1)"),
        "{}",
        stdout(&regressed)
    );
    assert!(
        stderr(&regressed).contains("regression"),
        "{}",
        stderr(&regressed)
    );
}

#[test]
fn mapping_writes_the_csv_to_a_file_or_the_console() {
    let file = std::env::temp_dir().join("dtsx-inspect-mapping-test.csv");
    let file_arg = file.to_str().expect("UTF-8 path");

    let written = run(&["mapping", "tests/fixtures/columns_sql", "--out", file_arg]);
    let printed = run(&["mapping", "tests/fixtures/columns_sql"]);

    assert!(written.status.success(), "{}", stderr(&written));
    let csv = std::fs::read_to_string(&file).expect("CSV written");
    std::fs::remove_file(&file).expect("cleanup");
    assert!(csv.starts_with("\u{feff}Project;Package;"), "{csv}");
    assert!(csv.contains("Entrepot : dbo.Archive.email"), "{csv}");
    assert!(stdout(&written).is_empty(), "{}", stdout(&written));
    assert_eq!(stdout(&printed), csv);
}
