//! GE-018: `shin new` and the `project.new` tool write the same project,
//! and the CLI reports bad names and non-empty target directories.

use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};

use serde_json::json;
use shin::headless::Headless;
use shin::tool::{ToolOutput, ToolRegistry};

/// A fresh, missing directory under the system temp dir.
fn temp_dir(label: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("shin-cli-test-{label}-{}", std::process::id()));
    let _ = fs::remove_dir_all(&dir);
    dir
}

/// Runs the `shin` binary with `args`.
fn shin(args: &[&str]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_shin"))
        .args(args)
        .output()
        .expect("run the `shin` binary")
}

/// `files` relative to `root`, as `/`-separated strings.
fn relative(root: &Path, files: impl IntoIterator<Item = PathBuf>) -> Vec<String> {
    files
        .into_iter()
        .map(|f| {
            let rel = f
                .strip_prefix(root)
                .unwrap_or_else(|_| panic!("{} is not under {}", f.display(), root.display()));
            rel.components()
                .map(|c| c.as_os_str().to_string_lossy().into_owned())
                .collect::<Vec<_>>()
                .join("/")
        })
        .collect()
}

#[test]
fn cli_and_tool_write_the_same_files() -> anyhow::Result<()> {
    let cli_parent = temp_dir("cli");
    let out = shin(&["new", "demo-game", "--dir", cli_parent.to_str().unwrap()]);
    assert!(
        out.status.success(),
        "shin new failed: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    let cli_files = String::from_utf8(out.stdout)?
        .lines()
        .map(PathBuf::from)
        .collect::<Vec<_>>();
    let cli_files = relative(&cli_parent.join("demo-game"), cli_files);

    let tool_parent = temp_dir("tool");
    let mut engine = Headless::new(16, 16)?;
    let output = ToolRegistry::with_engine_tools().call(
        &mut engine,
        "project.new",
        json!({ "name": "demo-game", "dir": tool_parent }),
    )?;
    let ToolOutput::Json(files) = output else {
        panic!("expected JSON output, got {output:?}");
    };
    let tool_files: Vec<PathBuf> = serde_json::from_value(files)?;
    let tool_files = relative(&tool_parent.join("demo-game"), tool_files);

    eprintln!("files: {cli_files:?}");
    assert_eq!(cli_files, tool_files);
    assert!(cli_files.contains(&"shin.toml".to_string()));
    assert!(cli_files.contains(&"src/main.rs".to_string()));
    for file in &cli_files {
        let a = fs::read(cli_parent.join("demo-game").join(file))?;
        let b = fs::read(tool_parent.join("demo-game").join(file))?;
        assert_eq!(a, b, "{file} differs between CLI and tool");
    }

    fs::remove_dir_all(&cli_parent)?;
    fs::remove_dir_all(&tool_parent)?;
    Ok(())
}

#[test]
fn non_empty_target_dir_is_an_error_naming_it() -> anyhow::Result<()> {
    let parent = temp_dir("non-empty");
    let target = parent.join("taken");
    fs::create_dir_all(&target)?;
    fs::write(target.join("keep.txt"), "mine")?;

    let out = shin(&["new", "taken", "--dir", parent.to_str().unwrap()]);
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(!out.status.success(), "expected failure; stderr: {stderr}");
    assert!(
        stderr.contains(&target.display().to_string()),
        "error does not name {}: {stderr}",
        target.display()
    );
    assert_eq!(fs::read_dir(&target)?.count(), 1, "nothing was written");

    fs::remove_dir_all(&parent)?;
    Ok(())
}

#[test]
fn invalid_names_are_errors() {
    let parent = temp_dir("bad-name");
    for name in ["my game", "2fast"] {
        let out = shin(&["new", name, "--dir", parent.to_str().unwrap()]);
        let stderr = String::from_utf8_lossy(&out.stderr);
        assert!(!out.status.success(), "`{name}` accepted");
        assert!(stderr.contains("invalid project name"), "{stderr}");
    }
    assert!(!parent.exists(), "nothing was written");
}
