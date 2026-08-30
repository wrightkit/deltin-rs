use deltin_rs::api::inspect_path;
use std::path::PathBuf;

fn project_dir(name: &str) -> PathBuf {
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("target")
        .join(format!("api-{name}-{}", std::process::id()));
    std::fs::create_dir_all(&path).unwrap();
    path
}

#[test]
fn inspect_facade_returns_queries_without_cli_types() {
    let root = project_dir("inspect");
    let file = root.join("main.del");
    let source = "rule: \"main\" Event.OngoingGlobal { define local = 1; local += 1; }\n";
    std::fs::write(&file, source).unwrap();

    let offset = source.find("local +=").unwrap() as u32;
    let report = inspect_path(&file, &file, offset);
    assert!(report.file.is_some());
    assert!(report.resolution.is_some());

    std::fs::remove_dir_all(root).unwrap();
}
