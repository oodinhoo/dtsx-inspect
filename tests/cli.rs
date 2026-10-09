use std::process::{Command, Output};

fn run(args: &[&str]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_dtsx-inspect"))
        .args(args)
        .current_dir(env!("CARGO_MANIFEST_DIR"))
        .output()
        .expect("lancement du binaire")
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
    let value: serde_json::Value = serde_json::from_slice(&out.stdout).expect("JSON valide");
    assert_eq!(value["packages"][0]["name"], "Chargement");
}

#[test]
fn never_prints_secrets() {
    for format in ["md", "json"] {
        let out = run(&["inventory", "tests/fixtures", "--format", format]);

        assert!(stdout(&out).contains("Demo_Minimal"), "{format}");
        assert!(
            !stdout(&out).contains("s3cr3t"),
            "fuite sur stdout en {format}"
        );
        assert!(
            !stderr(&out).contains("s3cr3t"),
            "fuite sur stderr en {format}"
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
    assert!(!text.contains("s3cr3t"), "fuite de secret");
    assert!(
        !text.contains("SELECT c.id"),
        "le texte SQL ne doit pas être affiché"
    );
}

#[test]
fn lineage_reports_broken_files() {
    let out = run(&["lineage", "tests/fixtures/broken"]);

    assert_eq!(out.status.code(), Some(1));
    assert!(stderr(&out).contains("corrompu.dtsx"), "{}", stderr(&out));
}
