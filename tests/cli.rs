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
fn report_writes_the_html_report_to_a_file() {
    let file = std::env::temp_dir().join("dtsx-inspect-report-test.html");
    let file_arg = file.to_str().expect("UTF-8 path");

    let out = run(&["report", "examples/demo", "--out", file_arg]);

    assert!(out.status.success(), "{}", stderr(&out));
    let page = std::fs::read_to_string(&file).expect("report written");
    std::fs::remove_file(&file).expect("cleanup");
    assert!(page.contains("10_Extract_Customers"), "package missing");
    assert!(!page.contains("s3cr3t"), "secret leak");
    assert!(stdout(&out).is_empty(), "{}", stdout(&out));
}

/// Marker planted in `tests/fixtures/canary` in every form a package or a script can hold
/// a secret: connection strings, parameters, variables, expressions, SQL, UTF-16 script.
const CANARY: &str = "CANARY_";
const CANARY_DIR: &str = "tests/fixtures/canary";

#[test]
fn no_output_contains_the_canary_secret() {
    let html = std::env::temp_dir().join("dtsx-inspect-canary.html");
    let csv = std::env::temp_dir().join("dtsx-inspect-canary.csv");
    let (html_arg, csv_arg) = (html.to_str().expect("UTF-8"), csv.to_str().expect("UTF-8"));
    let runs: [&[&str]; 11] = [
        &["inventory", CANARY_DIR],
        &["inventory", CANARY_DIR, "--format", "json"],
        &["lineage", CANARY_DIR],
        &["lint", CANARY_DIR],
        &["calls", CANARY_DIR],
        &["columns", CANARY_DIR, "--of", "dbo.Target"],
        &["mapping", CANARY_DIR],
        &["mapping", CANARY_DIR, "--out", csv_arg],
        &["order", CANARY_DIR],
        &["diff", CANARY_DIR, CANARY_DIR],
        &["report", CANARY_DIR, "--out", html_arg],
    ];

    for args in runs {
        let out = run(args);
        assert!(!stdout(&out).contains(CANARY), "leak on stdout: {args:?}");
        assert!(!stderr(&out).contains(CANARY), "leak on stderr: {args:?}");
        // Lint exits with 1 on errors: the leak check matters, not the verdict.
        if args[0] != "lint" {
            assert!(out.status.success(), "{args:?}: {}", stderr(&out));
        }
    }
    for file in [&html, &csv] {
        let text = std::fs::read_to_string(file).expect("file written");
        std::fs::remove_file(file).expect("cleanup");
        assert!(!text.contains(CANARY), "leak in {}", file.display());
        assert!(
            text.contains("Vault_Load"),
            "package missing from {}",
            file.display()
        );
    }
}

/// The canary test only proves something while the marker is really in the fixtures.
#[test]
fn canary_fixtures_hold_the_marker_in_every_form() {
    let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join(CANARY_DIR);
    let package = std::fs::read_to_string(dir.join("Vault_Load.dtsx")).expect("package");
    let script = std::fs::read(dir.join("Rotate.sql")).expect("script");
    let (pairs, _) = script[2..].as_chunks::<2>();
    let utf16: Vec<u16> = pairs.iter().map(|&p| u16::from_le_bytes(p)).collect();

    assert_eq!(package.matches(CANARY).count(), 12, "{package}");
    assert!(script.starts_with(&[0xFF, 0xFE]), "UTF-16 LE with BOM");
    assert!(String::from_utf16_lossy(&utf16).contains("CANARY_utf16"));
    assert!(std::fs::read_to_string(dir.join("Vault.conmgr"))
        .expect("conmgr")
        .contains("CANARY_conmgr"));
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
