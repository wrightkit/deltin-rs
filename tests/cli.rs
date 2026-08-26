//! Black-box CLI contracts: command migration, output boundaries, and exits.

use serde_json::Value;
use std::path::PathBuf;
use std::process::{Command, Output};

fn del_rs_bin() -> Command {
    Command::new(env!("CARGO_BIN_EXE_del-rs"))
}

fn sample() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/corpus/highlevel/enum-basic.del")
}

fn invalid_sample() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/cli,source-case.del")
}

fn missing_sample() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("target/cli-input-does-not-exist.del")
}

fn run(args: &[&str]) -> Output {
    del_rs_bin().args(args).output().unwrap()
}

fn json(stdout: &[u8]) -> Value {
    serde_json::from_slice(stdout).expect("CLI stdout must be one JSON document")
}

#[test]
fn help_exposes_task_oriented_surface_and_classifications() {
    let out = run(&["--help"]);
    assert_eq!(out.status.code(), Some(0));
    let help = String::from_utf8_lossy(&out.stdout);
    assert!(help.contains("check"));
    assert!(help.contains("inspect"));
    assert!(help.contains("support"));
    assert!(help.contains("dev"));
    assert!(help.contains("maintainer"));
    assert!(!help.contains("parse <"));
    assert!(!help.contains("matrix <"));
}

#[test]
fn developer_commands_and_legacy_aliases_preserve_machine_capability() {
    let canonical = run(&["dev", "hir", "--json", sample().to_str().unwrap()]);
    assert_eq!(
        canonical.status.code(),
        Some(0),
        "{}",
        String::from_utf8_lossy(&canonical.stderr)
    );
    assert_eq!(json(&canonical.stdout)["command"], "hir");

    let legacy = run(&["hir", "--json", sample().to_str().unwrap()]);
    assert_eq!(
        legacy.status.code(),
        Some(0),
        "{}",
        String::from_utf8_lossy(&legacy.stderr)
    );
    assert_eq!(json(&legacy.stdout)["command"], "hir");
}

#[test]
fn check_json_stdout_is_pure_and_schema_is_preserved() {
    let out = run(&["check", "--json", sample().to_str().unwrap()]);
    assert_eq!(out.status.code(), Some(0));
    assert!(
        out.stderr.is_empty(),
        "stderr: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    let doc = json(&out.stdout);
    assert_eq!(doc["command"], "check");
    assert_eq!(doc["phase"], "all");
    assert!(doc["diagnostics"].is_array());
    assert!(doc["summary"]["errors"].is_number());
}

#[test]
fn del_debug_does_not_pollute_machine_json_stderr() {
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("tests/corpus/projects/modules/Debug Camera.del");
    let out = del_rs_bin()
        .args(["check", "--json", path.to_str().unwrap()])
        .env("DEL_DEBUG", "1")
        .output()
        .unwrap();
    assert!(out.status.code() == Some(0) || out.status.code() == Some(1));
    assert!(
        out.stderr.is_empty(),
        "stderr: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    assert_eq!(json(&out.stdout)["command"], "check");
}

#[test]
fn parse_and_hir_exit_one_for_source_errors() {
    for args in [
        vec!["parse", invalid_sample().to_str().unwrap()],
        vec!["dev", "hir", invalid_sample().to_str().unwrap()],
    ] {
        let out = run(&args);
        assert_eq!(
            out.status.code(),
            Some(1),
            "stderr: {}",
            String::from_utf8_lossy(&out.stderr)
        );
    }
}

#[test]
fn inspect_propagates_diagnostics_but_keeps_best_effort_exit_zero() {
    let invalid = invalid_sample();
    let path = invalid.to_str().unwrap();
    let out = run(&["inspect", "--json", path, "1:1"]);
    assert_eq!(
        out.status.code(),
        Some(0),
        "stderr: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    assert!(out.stderr.is_empty());
    let doc = json(&out.stdout);
    assert_eq!(doc["command"], "inspect");
    assert!(doc["diagnostics"]
        .as_array()
        .unwrap()
        .iter()
        .any(|d| d["severity"] == "Error"));
    assert!(doc["summary"]["errors"].as_u64().unwrap() > 0);

    let human = run(&["inspect", path, "1:1"]);
    assert_eq!(human.status.code(), Some(0));
    assert!(String::from_utf8_lossy(&human.stderr).contains("PR"));
    assert!(String::from_utf8_lossy(&human.stdout).contains("query exit remains successful"));
}

#[test]
fn inspect_rejects_zero_negative_and_out_of_range_positions() {
    let path = sample();
    for position in ["0:1", "1:0", "-1:1", "1:-1", "999:1", "1:999"] {
        let out = run(&["inspect", path.to_str().unwrap(), position]);
        assert_eq!(
            out.status.code(),
            Some(2),
            "position={position}, stderr: {}",
            String::from_utf8_lossy(&out.stderr)
        );
        let stderr = String::from_utf8_lossy(&out.stderr);
        assert!(stderr.contains("del-rs:"));
        assert!(!stderr.to_ascii_lowercase().contains("panicked"));
        assert!(out.stdout.is_empty());
    }
}

#[test]
fn parse_missing_input_is_io_exit_four() {
    let out = run(&["parse", missing_sample().to_str().unwrap()]);
    assert_eq!(out.status.code(), Some(4));
    assert!(out.stdout.is_empty());
    assert!(!String::from_utf8_lossy(&out.stderr).contains("PJ002"));
}

#[test]
fn check_missing_input_is_io_exit_four() {
    let out = run(&["check", missing_sample().to_str().unwrap()]);
    assert_eq!(out.status.code(), Some(4));
    assert!(out.stdout.is_empty());
    assert!(!String::from_utf8_lossy(&out.stderr).contains("PJ002"));
}

#[test]
fn dev_hir_missing_input_is_io_exit_four() {
    let out = run(&["dev", "hir", missing_sample().to_str().unwrap()]);
    assert_eq!(out.status.code(), Some(4));
    assert!(out.stdout.is_empty());
    assert!(!String::from_utf8_lossy(&out.stderr).contains("PJ002"));
}

#[test]
fn inspect_missing_input_is_io_exit_four() {
    let out = run(&["inspect", missing_sample().to_str().unwrap(), "1:1"]);
    assert_eq!(out.status.code(), Some(4));
    assert!(out.stdout.is_empty());
    assert!(!String::from_utf8_lossy(&out.stderr).contains("PJ002"));
}

#[test]
fn support_is_stable_and_matrix_remains_a_compatibility_alias() {
    let support = run(&["support", "--json"]);
    assert_eq!(support.status.code(), Some(0));
    let document = json(&support.stdout);
    assert_eq!(document["command"], "support");
    assert_eq!(
        support.status.code(),
        Some(if document["valid"] == true { 0 } else { 1 })
    );

    let matrix = run(&["matrix", "--check"]);
    assert_eq!(matrix.status.code(), Some(0));
    assert!(String::from_utf8_lossy(&matrix.stdout).contains("support matrix valid"));
}

#[test]
fn maintainer_compatibility_and_legacy_alias_preserve_report_schema() {
    for args in [
        vec!["maintainer", "compatibility", "--json"],
        vec!["compatibility", "--json"],
    ] {
        let out = run(&args);
        assert_eq!(
            out.status.code(),
            Some(0),
            "{}",
            String::from_utf8_lossy(&out.stderr)
        );
        let doc = json(&out.stdout);
        assert_eq!(doc["schema"], 1);
        assert!(doc["summary"]["matched"].is_number());
        assert!(doc["summary"]["unexpected_regressions"].is_number());
        let expected_exit = if doc["summary"]["unexpected_regressions"] == 0 {
            0
        } else {
            1
        };
        assert_eq!(out.status.code(), Some(expected_exit));
    }
}

#[test]
fn completions_are_generated_for_supported_static_shells() {
    for shell in ["bash", "zsh", "fish", "powershell"] {
        let out = run(&["completion", shell]);
        assert_eq!(
            out.status.code(),
            Some(0),
            "shell={shell}: {}",
            String::from_utf8_lossy(&out.stderr)
        );
        assert!(out.stderr.is_empty());
        let completion = String::from_utf8_lossy(&out.stdout);
        assert!(completion.contains("check"), "shell={shell}");
        assert!(completion.contains("support"), "shell={shell}");
    }
}

#[test]
fn github_actions_annotations_escape_source_properties_and_write_summary() {
    let summary = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join(format!("target/cli-summary-{}.md", std::process::id()));
    let _ = std::fs::remove_file(&summary);
    let out = del_rs_bin()
        .args([
            "check",
            invalid_sample().to_str().unwrap(),
            "--presentation",
            "github-actions",
        ])
        .env("GITHUB_ACTIONS", "true")
        .env("GITHUB_STEP_SUMMARY", &summary)
        .output()
        .unwrap();
    assert_eq!(out.status.code(), Some(1));
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(stderr.contains("::error "), "stderr: {stderr}");
    assert!(stderr.contains("%2C"), "stderr: {stderr}");
    assert!(std::fs::read_to_string(&summary)
        .unwrap()
        .contains("### del-rs"));
    std::fs::remove_file(summary).unwrap();
}

#[test]
fn explicit_plain_presentation_isolated_from_github_environment() {
    let out = del_rs_bin()
        .args([
            "check",
            invalid_sample().to_str().unwrap(),
            "--presentation",
            "plain",
        ])
        .env("GITHUB_ACTIONS", "true")
        .env_remove("GITHUB_STEP_SUMMARY")
        .output()
        .unwrap();
    assert_eq!(out.status.code(), Some(1));
    assert!(!String::from_utf8_lossy(&out.stderr).contains("::error"));
    assert!(!String::from_utf8_lossy(&out.stderr).contains("\x1b["));
}

#[test]
fn no_color_is_honored_unless_color_is_explicitly_forced() {
    let automatic = del_rs_bin()
        .args([
            "check",
            invalid_sample().to_str().unwrap(),
            "--presentation",
            "terminal",
            "--color",
            "auto",
        ])
        .env("NO_COLOR", "1")
        .output()
        .unwrap();
    assert!(!String::from_utf8_lossy(&automatic.stderr).contains("\x1b["));

    let forced = del_rs_bin()
        .args([
            "check",
            invalid_sample().to_str().unwrap(),
            "--presentation",
            "terminal",
            "--color",
            "always",
        ])
        .env("NO_COLOR", "1")
        .output()
        .unwrap();
    assert!(String::from_utf8_lossy(&forced.stderr).contains("\x1b["));
}

#[test]
fn machine_json_overrides_workflow_presentation() {
    let out = del_rs_bin()
        .args([
            "check",
            "--json",
            invalid_sample().to_str().unwrap(),
            "--presentation",
            "github-actions",
        ])
        .env("GITHUB_ACTIONS", "true")
        .output()
        .unwrap();
    assert_eq!(out.status.code(), Some(1));
    assert!(
        out.stderr.is_empty(),
        "stderr: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    assert_eq!(json(&out.stdout)["command"], "check");
    assert!(!String::from_utf8_lossy(&out.stdout).contains("::error"));
}

#[test]
fn usage_io_and_source_error_exit_codes_are_distinct() {
    assert_eq!(run(&["bogus-command"]).status.code(), Some(2));
    assert_eq!(
        run(&["parse", "target/no-such-file.del"]).status.code(),
        Some(4)
    );
    assert_eq!(
        run(&["check", invalid_sample().to_str().unwrap()])
            .status
            .code(),
        Some(1)
    );
}
