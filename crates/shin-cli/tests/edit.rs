//! GE-020: `shin edit <dir>` runs the editor binary on `<dir>`, passing
//! arguments after `--` through, and reports an editor it cannot run.

use std::fs;
use std::path::PathBuf;
use std::process::{Command, Output};

/// A fresh, missing directory under the system temp dir.
fn temp_dir(label: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("shin-cli-edit-{label}-{}", std::process::id()));
    let _ = fs::remove_dir_all(&dir);
    dir
}

/// Runs `shin` with `args` and `SHIN_EDITOR` set to `editor`.
fn shin_with_editor(editor: &str, args: &[&str]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_shin"))
        .env("SHIN_EDITOR", editor)
        .args(args)
        .output()
        .expect("run the `shin` binary")
}

#[test]
fn edit_runs_the_editor_with_dir_then_extra_args() {
    // Stand-in editor: `shin` itself. `shin edit new -- demo --dir <parent>`
    // runs `<editor> new demo --dir <parent>`, which creates a project, so
    // the project appearing proves the dir and the extra args were passed
    // in order and the exit status was propagated.
    let parent = temp_dir("forward");
    let out = shin_with_editor(
        env!("CARGO_BIN_EXE_shin"),
        &[
            "edit",
            "new",
            "--",
            "demo",
            "--dir",
            parent.to_str().unwrap(),
        ],
    );
    assert!(
        out.status.success(),
        "shin edit failed: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    assert!(parent.join("demo/shin.toml").is_file());
    let _ = fs::remove_dir_all(&parent);
}

#[test]
fn edit_reports_an_editor_it_cannot_run() {
    let missing = temp_dir("missing").join("shin-editor");
    let out = shin_with_editor(missing.to_str().unwrap(), &["edit", "."]);
    assert!(!out.status.success());
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(
        stderr.contains("could not run the editor") && stderr.contains("shin-editor"),
        "stderr: {stderr}"
    );
}

#[test]
fn edit_propagates_editor_failure() {
    // `shin edit bogus` with `shin` as the editor runs `shin bogus`, which
    // fails; `shin edit` must fail too.
    let out = shin_with_editor(env!("CARGO_BIN_EXE_shin"), &["edit", "bogus"]);
    assert!(!out.status.success());
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(
        stderr.contains("the editor exited with"),
        "stderr: {stderr}"
    );
}
