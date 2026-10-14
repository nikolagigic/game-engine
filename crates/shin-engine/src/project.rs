//! Game projects: the `shin.toml` manifest and scaffolding a new, runnable
//! project (`shin new`, the `project.new` tool). Owned by GE-018.
//!
//! A project is a normal Cargo package that depends on `shin-engine`, plus a
//! `shin.toml` the engine and tools read:
//!
//! ```toml
//! [project]
//! name = "demo-game"
//!
//! [window]
//! title = "demo-game"
//! width = 1280
//! height = 720
//! ```
//!
//! [`create`] writes the starter project: `Cargo.toml`, `shin.toml`,
//! `src/main.rs` (a lit cube on a ground slab with the fly camera),
//! `scenes/main.scene.ron` (the same scene as a scene file, the project's
//! entry scene), `assets/cube.gltf` + `assets/cube.bin`, `.mcp.json` (the
//! engine as MCP tools), `.gitignore`, and `README.md`.
//!
//! An open project (GE-026, [`Headless::open_project`]) is marked by the
//! [`ProjectRoot`] resource; the `project.open` and `project.info` tools
//! open and describe one.

use std::fs;
use std::path::{Component, Path, PathBuf};

use anyhow::Context as _;
use serde::{Deserialize, Serialize};
use serde_json::json;

use crate::headless::Headless;
use crate::scene::{CameraDesc, EntityDesc, LightDesc, SceneFile, TransformDesc};
use crate::tool::{ToolOutput, ToolRegistry, TypedTool};

/// Name of the project manifest in a project's root directory.
pub const MANIFEST: &str = "shin.toml";

/// Git URL of the engine, used by [`EngineDep::Git`] by default.
pub const ENGINE_GIT_URL: &str = "https://github.com/nikolagigic/shin-engine";

/// A game project, as described by its `shin.toml`.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Project {
    /// The project (and Cargo package) name.
    pub name: String,
    /// The game window.
    pub window: WindowSettings,
    /// The scene loaded at start-up, relative to the project root, e.g.
    /// `scenes/main.scene.ron`. [`Headless::open_project`] loads it.
    pub entry_scene: Option<PathBuf>,
}

/// The game window's initial title and size, the `[window]` table of
/// `shin.toml`.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct WindowSettings {
    /// Window title.
    pub title: String,
    /// Initial width in logical pixels.
    pub width: u32,
    /// Initial height in logical pixels.
    pub height: u32,
}

impl Default for WindowSettings {
    fn default() -> Self {
        Self {
            title: "shin".to_string(),
            width: 1280,
            height: 720,
        }
    }
}

/// The on-disk layout of `shin.toml`: `[project]` and `[window]` tables.
#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct ManifestFile {
    project: ProjectTable,
    window: WindowSettings,
}

/// The `[project]` table of `shin.toml`.
#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct ProjectTable {
    name: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    entry_scene: Option<PathBuf>,
}

impl Project {
    /// A project called `name` with a window titled `name` and the default
    /// size, and no entry scene.
    pub fn new(name: &str) -> Self {
        Self {
            name: name.to_string(),
            window: WindowSettings {
                title: name.to_string(),
                ..Default::default()
            },
            entry_scene: None,
        }
    }

    /// Reads `<dir>/shin.toml`.
    pub fn load(dir: &Path) -> anyhow::Result<Self> {
        let path = dir.join(MANIFEST);
        let text =
            fs::read_to_string(&path).with_context(|| format!("reading {}", path.display()))?;
        let file: ManifestFile =
            toml::from_str(&text).with_context(|| format!("parsing {}", path.display()))?;
        Ok(Self {
            name: file.project.name,
            window: file.window,
            entry_scene: file.project.entry_scene,
        })
    }

    /// Writes `<dir>/shin.toml`, replacing any existing one.
    pub fn save(&self, dir: &Path) -> anyhow::Result<()> {
        let path = dir.join(MANIFEST);
        fs::write(&path, self.to_toml()?).with_context(|| format!("writing {}", path.display()))
    }

    /// The `shin.toml` text for this project.
    fn to_toml(&self) -> anyhow::Result<String> {
        let file = ManifestFile {
            project: ProjectTable {
                name: self.name.clone(),
                entry_scene: self.entry_scene.clone(),
            },
            window: self.window.clone(),
        };
        Ok(toml::to_string(&file)?)
    }
}

/// The root directory of the open project, as an absolute, canonical path.
///
/// [`Headless::open_project`] inserts it as a resource. While it is set,
/// the `scene.*` tools resolve their paths with [`ProjectRoot::resolve`].
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ProjectRoot(pub PathBuf);

impl ProjectRoot {
    /// `path` joined onto the root and normalised (`.` and `..` removed
    /// without touching the file system, so the file need not exist).
    ///
    /// Fails, naming `path`, if the result is outside the root: a relative
    /// path that climbs out with `..`, or an absolute path elsewhere.
    pub fn resolve(&self, path: &Path) -> anyhow::Result<PathBuf> {
        let mut resolved = PathBuf::new();
        for component in self.0.join(path).components() {
            match component {
                Component::CurDir => {}
                Component::ParentDir => {
                    resolved.pop();
                }
                other => resolved.push(other),
            }
        }
        anyhow::ensure!(
            resolved.starts_with(&self.0),
            "path `{}` escapes the project root {}",
            path.display(),
            self.0.display()
        );
        Ok(resolved)
    }
}

/// Where a new project gets `shin-engine` from.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum EngineDep {
    /// A git repository, e.g. [`ENGINE_GIT_URL`].
    Git {
        /// Repository URL.
        url: String,
    },
    /// A local checkout: the `crates/shin-engine` directory. Relative paths
    /// are made absolute against the current directory.
    Path(PathBuf),
}

impl Default for EngineDep {
    fn default() -> Self {
        Self::Git {
            url: ENGINE_GIT_URL.to_string(),
        }
    }
}

/// What [`create`] should make.
#[derive(Clone, Debug)]
pub struct NewProjectOptions {
    /// Project and Cargo package name; see [`validate_name`].
    pub name: String,
    /// The project's root directory. It must not exist or be empty.
    pub dir: PathBuf,
    /// Where the project gets the engine from.
    pub engine: EngineDep,
}

/// Checks that `name` is usable as a Cargo package name: non-empty, ASCII
/// letters, digits, `-` and `_` only, not starting with a digit.
pub fn validate_name(name: &str) -> anyhow::Result<()> {
    anyhow::ensure!(!name.is_empty(), "project name is empty");
    if let Some(bad) = name
        .chars()
        .find(|c| !(c.is_ascii_alphanumeric() || *c == '-' || *c == '_'))
    {
        anyhow::bail!(
            "invalid project name `{name}`: {bad:?} is not allowed; \
             use ASCII letters, digits, `-` and `_`"
        );
    }
    anyhow::ensure!(
        !name.starts_with(|c: char| c.is_ascii_digit()),
        "invalid project name `{name}`: it must not start with a digit"
    );
    Ok(())
}

/// The starter `src/main.rs`. It reads its window settings from `shin.toml`
/// at run time, so it needs no substitution.
const MAIN_RS: &str = include_str!("project/template/main.rs");
/// The glTF cube the starter scene draws, copied into `assets/`.
const CUBE_GLTF: &[u8] = include_bytes!("../assets/test/cube.gltf");
/// The cube's vertex buffer, referenced by [`CUBE_GLTF`] as `cube.bin`.
const CUBE_BIN: &[u8] = include_bytes!("../assets/test/cube.bin");

/// Creates a new project in `opts.dir` and returns the files written, in
/// order, as paths under `opts.dir`.
///
/// Fails without writing anything if the name is invalid or `opts.dir`
/// exists and is not an empty directory.
pub fn create(opts: &NewProjectOptions) -> anyhow::Result<Vec<PathBuf>> {
    validate_name(&opts.name)?;
    let dir = &opts.dir;
    if dir.exists() {
        let mut entries = fs::read_dir(dir)
            .with_context(|| format!("{} exists and is not a directory", dir.display()))?;
        anyhow::ensure!(
            entries.next().is_none(),
            "{} already exists and is not empty",
            dir.display()
        );
    }

    let mut project = Project::new(&opts.name);
    project.entry_scene = Some(PathBuf::from(STARTER_SCENE));
    let files: [(&str, Vec<u8>); 9] = [
        ("Cargo.toml", cargo_toml(&opts.name, &opts.engine)?.into()),
        (MANIFEST, project.to_toml()?.into()),
        ("src/main.rs", MAIN_RS.into()),
        (STARTER_SCENE, starter_scene().to_ron()?.into()),
        ("assets/cube.gltf", CUBE_GLTF.into()),
        ("assets/cube.bin", CUBE_BIN.into()),
        (".mcp.json", MCP_JSON.into()),
        (".gitignore", "/target\n".into()),
        ("README.md", readme(&opts.name).into()),
    ];

    let mut written = Vec::with_capacity(files.len());
    for (relative, contents) in files {
        let path = dir.join(relative);
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent).with_context(|| format!("creating {}", parent.display()))?;
        }
        fs::write(&path, contents).with_context(|| format!("writing {}", path.display()))?;
        written.push(path);
    }
    log::info!("created project `{}` in {}", opts.name, dir.display());
    Ok(written)
}

/// Where [`create`] writes the starter scene, relative to the project root;
/// also the generated project's `entry_scene`.
pub const STARTER_SCENE: &str = "scenes/main.scene.ron";

/// The generated `.mcp.json`: an MCP client such as Claude Code, opened in
/// the project folder, starts `shin-mcp` on it and gets the engine as tools.
const MCP_JSON: &str = r#"{
  "mcpServers": {
    "shin": {
      "command": "shin-mcp",
      "args": ["--project", "."]
    }
  }
}
"#;

/// The starter scene as a scene file: the same lit cube on a ground slab
/// that `src/main.rs` builds in code, drawn with the project's
/// `assets/cube.gltf`.
fn starter_scene() -> SceneFile {
    let cube = |name: &str, transform: TransformDesc, color: [f32; 4]| EntityDesc {
        name: Some(name.to_string()),
        transform,
        mesh: Some("assets/cube.gltf".to_string()),
        color: Some(color),
        texture: None,
    };
    SceneFile {
        camera: Some(CameraDesc {
            position: [0.0, 2.0, 5.0],
            look_at: [0.0, 0.5, 0.0],
            fov_y_deg: 60.0,
        }),
        light: Some(LightDesc {
            direction: [-0.6, -1.0, -0.4],
            color: [1.0, 0.95, 0.85],
            ambient: [0.12, 0.13, 0.16],
        }),
        entities: vec![
            // Ground: the cube flattened into a slab whose top face is y = 0.
            cube(
                "ground",
                TransformDesc {
                    translation: [0.0, -0.25, 0.0],
                    scale: [20.0, 0.5, 20.0],
                    ..Default::default()
                },
                [0.18, 0.2, 0.17, 1.0],
            ),
            // The cube, turned a little so the light catches three faces.
            cube(
                "cube",
                TransformDesc {
                    translation: [0.0, 0.5, 0.0],
                    rotation_euler_deg: [0.0, 30.0, 0.0],
                    ..Default::default()
                },
                [0.8, 0.25, 0.1, 1.0],
            ),
        ],
        ..Default::default()
    }
}

/// The project's `Cargo.toml`. Besides the engine it depends on `wgpu`
/// (for `wgpu::Color`), `anyhow` and `env_logger`, at the versions the
/// engine itself pins.
fn cargo_toml(name: &str, engine: &EngineDep) -> anyhow::Result<String> {
    let engine = match engine {
        EngineDep::Git { url } => format!("{{ git = {} }}", toml_string(url)),
        EngineDep::Path(path) => {
            let path = std::path::absolute(path)
                .with_context(|| format!("resolving engine path {}", path.display()))?;
            let path = path
                .to_str()
                .with_context(|| format!("engine path {} is not UTF-8", path.display()))?;
            format!("{{ path = {} }}", toml_string(path))
        }
    };
    Ok(format!(
        r#"[package]
name = "{name}"
version = "0.1.0"
edition = "2024"
publish = false

[dependencies]
shin-engine = {engine}
wgpu = "30.0"
anyhow = "1.0"
env_logger = "0.11"

# Optimise dependencies even in debug builds so the game runs smoothly.
[profile.dev]
opt-level = 1

[profile.dev.package."*"]
opt-level = 3

# A standalone project, even when created inside another Cargo workspace.
[workspace]
"#
    ))
}

/// `s` as a quoted, escaped TOML string.
fn toml_string(s: &str) -> String {
    toml::Value::String(s.to_string()).to_string()
}

/// The project's `README.md`.
fn readme(name: &str) -> String {
    format!(
        r#"# {name}

A game made with [Shin]({ENGINE_GIT_URL}).

## Run

```sh
cargo run
```

A lit cube on a ground slab. The window title and size come from
`shin.toml`.

## Drive it with an agent

Open this folder in Claude Code and the engine is available as tools:
`.mcp.json` starts `shin-mcp --project .`, which loads `shin.toml` and the
entry scene, so an agent can inspect, edit, render, and save the scene.

## Controls

- W/S: forward and back, A/D: strafe, E/Q: up and down
- Hold the right mouse button to look around
- Escape: quit

## Layout

```
shin.toml       project name, window settings, and entry scene
src/main.rs     the game: scene setup and systems
scenes/         scene files (.scene.ron)
assets/         glTF meshes and textures
.mcp.json       the engine as MCP tools for agents
```
"#
    )
}

/// Arguments of `project.new`.
#[derive(Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
struct NewArgs {
    /// Project and Cargo package name: ASCII letters, digits, `-` and `_`,
    /// not starting with a digit.
    name: String,
    /// Parent directory; the project is created in `<dir>/<name>`.
    dir: PathBuf,
    /// Path to a local `crates/shin-engine` to depend on instead of the
    /// engine's git repository.
    engine_path: Option<PathBuf>,
}

/// Arguments of `project.open`.
#[derive(Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
struct OpenArgs {
    /// The project's root directory, the one containing `shin.toml`.
    dir: PathBuf,
}

/// `project.info` takes no arguments.
#[derive(Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
struct InfoArgs {}

/// The `project.info` description of the open project: `{ name, dir,
/// entry_scene, window: { title, width, height } }`. Re-reads `shin.toml`,
/// so edits made since opening show up.
fn info(engine: &Headless) -> anyhow::Result<serde_json::Value> {
    let root = engine
        .resources
        .get::<ProjectRoot>()
        .context("no project is open; call project.open first")?;
    let project = Project::load(&root.0)?;
    Ok(json!({
        "name": project.name,
        "dir": root.0,
        "entry_scene": project.entry_scene,
        "window": project.window,
    }))
}

/// Registers `project.new`, `project.open`, and `project.info`.
pub fn register_tools(reg: &mut ToolRegistry) {
    reg.register(TypedTool::new(
        "project.open",
        "Opens the project in `dir` (the folder with shin.toml): replaces every entity, roots \
         asset and scene paths at the project, resizes the render target to the project's \
         window size, and loads its entry scene if it has one. Returns the same as \
         project.info.",
        |engine: &mut Headless, args: OpenArgs| {
            engine.load_project(&args.dir)?;
            Ok(ToolOutput::Json(info(engine)?))
        },
    ));
    reg.register(TypedTool::new(
        "project.info",
        "Describes the open project: { name, dir, entry_scene, window: { title, width, \
         height } }, read from its shin.toml. Fails if no project is open.",
        |engine: &mut Headless, _: InfoArgs| Ok(ToolOutput::Json(info(engine)?)),
    ));
    reg.register(TypedTool::new(
        "project.new",
        "Creates a runnable game project in `<dir>/<name>` (Cargo.toml, shin.toml, \
         src/main.rs with a lit cube scene, assets) and returns the list of files written. \
         Fails if that directory exists and is not empty.",
        |_: &mut Headless, args: NewArgs| {
            let opts = NewProjectOptions {
                dir: args.dir.join(&args.name),
                name: args.name,
                engine: args.engine_path.map(EngineDep::Path).unwrap_or_default(),
            };
            let files = create(&opts)?;
            Ok(ToolOutput::Json(serde_json::to_value(files)?))
        },
    ));
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::asset::Assets;

    /// A fresh, empty-or-missing directory under the system temp dir.
    fn temp_dir(label: &str) -> PathBuf {
        let dir =
            std::env::temp_dir().join(format!("shin-project-test-{label}-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        dir
    }

    #[test]
    fn valid_names_pass() {
        for name in ["demo", "demo-game", "my_game2", "A"] {
            validate_name(name).unwrap_or_else(|e| panic!("`{name}`: {e}"));
        }
    }

    #[test]
    fn invalid_names_are_rejected() {
        for name in ["", "my game", "2fast", "caf\u{e9}", "a/b"] {
            assert!(validate_name(name).is_err(), "`{name}` accepted");
        }
        let err = validate_name("9lives").unwrap_err().to_string();
        assert!(err.contains("start with a digit"), "{err}");
        let err = validate_name("my game").unwrap_err().to_string();
        assert!(err.contains("my game"), "{err}");
    }

    #[test]
    fn create_rejects_invalid_name_without_writing() {
        let dir = temp_dir("bad-name");
        let opts = NewProjectOptions {
            name: "my game".to_string(),
            dir: dir.clone(),
            engine: EngineDep::default(),
        };
        let err = create(&opts).unwrap_err().to_string();
        assert!(err.contains("invalid project name"), "{err}");
        assert!(!dir.exists());
    }

    #[test]
    fn create_rejects_non_empty_dir_naming_it() -> anyhow::Result<()> {
        let dir = temp_dir("non-empty");
        fs::create_dir_all(&dir)?;
        fs::write(dir.join("keep.txt"), "mine")?;
        let opts = NewProjectOptions {
            name: "demo".to_string(),
            dir: dir.clone(),
            engine: EngineDep::default(),
        };
        let err = create(&opts).unwrap_err().to_string();
        assert!(err.contains(&dir.display().to_string()), "{err}");
        assert!(err.contains("not empty"), "{err}");
        assert_eq!(fs::read_dir(&dir)?.count(), 1, "nothing else was written");
        fs::remove_dir_all(&dir)?;
        Ok(())
    }

    #[test]
    fn create_writes_the_layout_into_an_empty_dir() -> anyhow::Result<()> {
        let dir = temp_dir("layout");
        fs::create_dir_all(&dir)?;
        let opts = NewProjectOptions {
            name: "demo-game".to_string(),
            dir: dir.clone(),
            engine: EngineDep::default(),
        };
        let files = create(&opts)?;
        let relative: Vec<_> = files
            .iter()
            .map(|f| f.strip_prefix(&dir).unwrap().to_str().unwrap().to_string())
            .collect();
        assert_eq!(
            relative,
            [
                "Cargo.toml",
                "shin.toml",
                "src/main.rs",
                "scenes/main.scene.ron",
                "assets/cube.gltf",
                "assets/cube.bin",
                ".mcp.json",
                ".gitignore",
                "README.md",
            ]
        );
        let cargo = fs::read_to_string(dir.join("Cargo.toml"))?;
        assert!(cargo.contains(r#"name = "demo-game""#), "{cargo}");
        assert!(
            cargo.contains(&format!(r#"git = "{ENGINE_GIT_URL}""#)),
            "{cargo}"
        );
        let project = Project::load(&dir)?;
        assert_eq!(project.entry_scene, Some(PathBuf::from(STARTER_SCENE)));
        let scene = SceneFile::from_ron(&fs::read_to_string(dir.join(STARTER_SCENE))?)?;
        assert_eq!(scene, starter_scene());
        fs::remove_dir_all(&dir)?;
        Ok(())
    }

    #[test]
    fn generated_mcp_json_runs_shin_mcp_on_the_project() -> anyhow::Result<()> {
        let dir = temp_dir("mcp-json");
        create(&NewProjectOptions {
            name: "demo".to_string(),
            dir: dir.clone(),
            engine: EngineDep::default(),
        })?;
        let config: serde_json::Value =
            serde_json::from_str(&fs::read_to_string(dir.join(".mcp.json"))?)?;
        assert_eq!(
            config,
            json!({ "mcpServers": { "shin": {
                "command": "shin-mcp",
                "args": ["--project", "."],
            } } })
        );
        let readme = fs::read_to_string(dir.join("README.md"))?;
        assert!(readme.contains("Claude Code"), "{readme}");
        fs::remove_dir_all(&dir)?;
        Ok(())
    }

    #[test]
    fn project_root_resolves_inside_and_rejects_escapes() -> anyhow::Result<()> {
        let root = ProjectRoot(PathBuf::from("/games/demo"));
        assert_eq!(
            root.resolve(Path::new("scenes/main.scene.ron"))?,
            Path::new("/games/demo/scenes/main.scene.ron")
        );
        assert_eq!(
            root.resolve(Path::new("./a/../b.scene.ron"))?,
            Path::new("/games/demo/b.scene.ron")
        );
        assert_eq!(
            root.resolve(Path::new("/games/demo/x.scene.ron"))?,
            Path::new("/games/demo/x.scene.ron")
        );
        for bad in [
            "../other/x.scene.ron",
            "scenes/../../x.scene.ron",
            "/etc/x.scene.ron",
            "..",
        ] {
            let err = root.resolve(Path::new(bad)).unwrap_err().to_string();
            assert!(err.contains(&format!("`{bad}`")), "{err}");
            assert!(err.contains("escapes the project root"), "{err}");
        }
        // A sibling directory sharing the root's name as a prefix is outside.
        assert!(root.resolve(Path::new("../demo2/x.scene.ron")).is_err());
        Ok(())
    }

    #[test]
    fn project_tools_open_describe_and_confine_scene_paths() -> anyhow::Result<()> {
        let dir = temp_dir("tools");
        create(&NewProjectOptions {
            name: "tool-game".to_string(),
            dir: dir.clone(),
            engine: EngineDep::default(),
        })?;
        let reg = ToolRegistry::with_engine_tools();
        for name in ["project.open", "project.info"] {
            let info = reg
                .list()
                .into_iter()
                .find(|t| t.name == name)
                .unwrap_or_else(|| panic!("{name} is not registered"));
            assert!(!info.description.is_empty(), "{name} has no description");
        }

        let mut engine = Headless::new(8, 8)?;
        let err = reg
            .call(&mut engine, "project.info", json!({}))
            .unwrap_err();
        assert!(err.to_string().contains("no project is open"), "{err}");

        let ToolOutput::Json(opened) =
            reg.call(&mut engine, "project.open", json!({ "dir": dir }))?
        else {
            panic!("expected JSON");
        };
        let root = dir.canonicalize()?;
        assert_eq!(opened["name"], "tool-game");
        assert_eq!(opened["dir"], json!(root));
        assert_eq!(opened["entry_scene"], STARTER_SCENE);
        assert_eq!(
            opened["window"],
            json!({ "title": "tool-game", "width": 1280, "height": 720 })
        );
        assert_eq!((engine.target.width, engine.target.height), (1280, 720));
        assert_eq!(engine.world.len(), 2, "the entry scene's entities");
        let ToolOutput::Json(info) = reg.call(&mut engine, "project.info", json!({}))? else {
            panic!("expected JSON");
        };
        assert_eq!(info, opened);

        // Relative scene paths land under the project root.
        reg.call(
            &mut engine,
            "scene.save",
            json!({ "path": "scenes/copy.scene.ron" }),
        )?;
        assert!(root.join("scenes/copy.scene.ron").is_file());
        reg.call(
            &mut engine,
            "scene.load",
            json!({ "path": "scenes/copy.scene.ron" }),
        )?;
        assert_eq!(engine.world.len(), 2);

        // Escaping the root is an error naming the path, for both tools.
        for tool in ["scene.save", "scene.load"] {
            let err = reg
                .call(&mut engine, tool, json!({ "path": "../escape.scene.ron" }))
                .unwrap_err()
                .to_string();
            assert!(err.contains("../escape.scene.ron"), "{tool}: {err}");
        }
        assert!(!dir.join("../escape.scene.ron").exists());

        // A directory without shin.toml fails and leaves the project open.
        let err = reg
            .call(
                &mut engine,
                "project.open",
                json!({ "dir": dir.join("scenes") }),
            )
            .unwrap_err();
        assert!(format!("{err:#}").contains(MANIFEST), "{err:#}");
        assert_eq!(
            engine.resources.get::<ProjectRoot>(),
            Some(&ProjectRoot(root))
        );
        assert_eq!(engine.world.len(), 2);

        fs::remove_dir_all(&dir)?;
        Ok(())
    }

    #[test]
    fn broken_entry_scene_leaves_the_engine_unchanged() -> anyhow::Result<()> {
        let dir = temp_dir("broken-entry");
        create(&NewProjectOptions {
            name: "broken".to_string(),
            dir: dir.clone(),
            engine: EngineDep::default(),
        })?;
        fs::write(dir.join(STARTER_SCENE), "(version: 1, bogus: 3)")?;
        let mut engine = Headless::new(8, 8)?;
        let assets_root = engine.resources.expect::<Assets>().root().to_path_buf();
        let err = engine.load_project(&dir).unwrap_err();
        assert!(format!("{err:#}").contains("bogus"), "{err:#}");
        assert!(engine.resources.get::<ProjectRoot>().is_none());
        assert_eq!(engine.resources.expect::<Assets>().root(), assets_root);
        assert_eq!((engine.target.width, engine.target.height), (8, 8));
        fs::remove_dir_all(&dir)?;
        Ok(())
    }

    #[test]
    fn open_project_sizes_the_target_from_shin_toml() -> anyhow::Result<()> {
        let dir = temp_dir("open");
        create(&NewProjectOptions {
            name: "sized".to_string(),
            dir: dir.clone(),
            engine: EngineDep::default(),
        })?;
        let mut project = Project::load(&dir)?;
        project.window.width = 64;
        project.window.height = 32;
        project.save(&dir)?;
        let mut engine = Headless::open_project(&dir)?;
        assert_eq!((engine.target.width, engine.target.height), (64, 32));
        assert_eq!(engine.world.len(), 2);
        assert_eq!(engine.render()?.dimensions(), (64, 32));
        fs::remove_dir_all(&dir)?;
        Ok(())
    }

    #[test]
    fn manifest_round_trips() -> anyhow::Result<()> {
        let dir = temp_dir("manifest");
        fs::create_dir_all(&dir)?;
        let mut project = Project::new("demo");
        project.window.width = 800;
        project.entry_scene = Some(PathBuf::from("scenes/main.scene.ron"));
        project.save(&dir)?;
        assert_eq!(Project::load(&dir)?, project);

        let text = fs::read_to_string(dir.join(MANIFEST))?;
        assert!(
            text.contains("[project]") && text.contains("[window]"),
            "{text}"
        );
        fs::remove_dir_all(&dir)?;
        Ok(())
    }

    #[test]
    fn engine_path_is_absolute_and_escaped() -> anyhow::Result<()> {
        let toml = cargo_toml("x", &EngineDep::Path(PathBuf::from("rel/eng\"ine")))?;
        let parsed: toml::Table = toml.parse()?;
        let path = parsed["dependencies"]["shin-engine"]["path"]
            .as_str()
            .expect("path is a string");
        assert!(Path::new(path).is_absolute(), "{path}");
        assert!(path.ends_with("rel/eng\"ine"), "{path}");
        Ok(())
    }
}
