//! Launching games: natively, or through a compatibility tool (Proton) the way Steam does.
//!
//! For compatibility tools Steam reads each tool's `toolmanifest.vdf`: `commandline` (with
//! `%verb%`) says how to invoke it, and `require_tool_appid` names another tool to wrap it in.
//! Proton requires the Steam Linux Runtime (sniper), whose `_v2-entry-point` starts the
//! pressure-vessel container. The resulting command is
//!
//! ```text
//! <sniper>/_v2-entry-point --verb=waitforexitandrun -- <proton>/proton waitforexitandrun <game.exe> [args]
//! ```
//!
//! with the `STEAM_COMPAT_*` environment the `proton` script reads (see `proton` in
//! ValveSoftware/Proton). Custom tools such as GE-Proton in `compatibilitytools.d` follow the
//! same manifest format, so they work through the same path.

use crate::config::{self, Settings};
use crate::install::Runtime;
use crate::library::{AppManifest, LibraryFolder};
use crate::pics::{AppDetails, LaunchEntry};
use crate::vdf;
use std::path::{Path, PathBuf};

/// Steam Linux Runtime 3.0 (sniper).
pub const SNIPER_APP_ID: u32 = 1628350;
pub const PROTON_EXPERIMENTAL_APP_ID: u32 = 1493710;

/// Proton builds Valve publishes as Steam tool apps, newest first.
pub const STEAM_PROTONS: &[(u32, &str)] = &[
    (PROTON_EXPERIMENTAL_APP_ID, "Proton Experimental"),
    (3658110, "Proton 10.0"),
    (2805730, "Proton 9.0"),
    (2348590, "Proton 8.0"),
    (2180100, "Proton Hotfix"),
];

#[derive(Debug, thiserror::Error)]
pub enum LaunchError {
    #[error("{0} has no launch option for {1}")]
    NoLaunchEntry(String, &'static str),
    #[error("executable not found: {0}")]
    MissingExecutable(PathBuf),
    #[error("compatibility tool '{0}' is not installed")]
    ToolMissing(String),
    #[error("{0} has no usable toolmanifest.vdf")]
    BadTool(PathBuf),
    #[error("required tool app {0} is not installed")]
    RequiredToolMissing(u32),
    #[error("invalid launch options: {0}")]
    LaunchOptions(String),
    #[error("i/o: {0}")]
    Io(#[from] std::io::Error),
}

/// A compatibility tool available on this machine.
#[derive(Clone, Debug, PartialEq)]
pub struct CompatTool {
    /// Selection key: the Steam app id as a string, or `custom:<dir name>`.
    pub key: String,
    pub name: String,
    pub dir: PathBuf,
}

#[derive(Clone, Debug, PartialEq)]
pub struct ToolManifest {
    pub commandline: String,
    pub require_tool_appid: Option<u32>,
}

pub fn read_toolmanifest(dir: &Path) -> Option<ToolManifest> {
    let kv = vdf::parse(&std::fs::read_to_string(dir.join("toolmanifest.vdf")).ok()?)?;
    Some(ToolManifest {
        commandline: vdf::get_str(&kv, "commandline")?,
        require_tool_appid: vdf::get_u64(&kv, "require_tool_appid").map(|v| v as u32),
    })
}

/// Where an installed Steam tool app lives, searching every library.
pub fn installed_app_dir(settings: &Settings, app_id: u32) -> Option<PathBuf> {
    settings.libraries.iter().find_map(|lib| {
        let m = lib
            .read_manifest(app_id)
            .filter(AppManifest::is_installed)?;
        let dir = lib.install_path(&m.install_dir);
        dir.is_dir().then_some(dir)
    })
}

fn compat_tool_dirs() -> Vec<PathBuf> {
    let home = PathBuf::from(std::env::var_os("HOME").unwrap_or_default());
    vec![
        config::data_dir().join("compatibilitytools.d"),
        home.join(".local/share/Steam/compatibilitytools.d"),
        home.join(".steam/root/compatibilitytools.d"),
        home.join(".var/app/com.valvesoftware.Steam/.local/share/Steam/compatibilitytools.d"),
    ]
}

/// Display name from `compatibilitytool.vdf`, falling back to the directory name.
fn custom_tool_name(dir: &Path) -> String {
    let fallback = dir
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_default();
    std::fs::read_to_string(dir.join("compatibilitytool.vdf"))
        .ok()
        .and_then(|t| vdf::parse(&t))
        .and_then(|kv| {
            let tools = kv.get("compat_tools")?;
            vdf::children(Some(tools))
                .into_iter()
                .find_map(|(_, t)| vdf::get_str(t, "display_name"))
        })
        .unwrap_or(fallback)
}

/// Every installed compatibility tool: Valve's Proton apps first, then custom tools.
pub fn installed_tools(settings: &Settings) -> Vec<CompatTool> {
    let mut out: Vec<CompatTool> = STEAM_PROTONS
        .iter()
        .filter_map(|&(app_id, name)| {
            Some(CompatTool {
                key: app_id.to_string(),
                name: name.to_string(),
                dir: installed_app_dir(settings, app_id)?,
            })
        })
        .collect();
    let mut seen = std::collections::HashSet::new();
    for base in compat_tool_dirs() {
        let Ok(entries) = std::fs::read_dir(&base) else {
            continue;
        };
        let mut dirs: Vec<PathBuf> = entries
            .flatten()
            .map(|e| e.path())
            .filter(|p| p.join("toolmanifest.vdf").is_file())
            .collect();
        dirs.sort();
        for dir in dirs {
            let Some(name) = dir.file_name().map(|n| n.to_string_lossy().into_owned()) else {
                continue;
            };
            if seen.insert(name.clone()) {
                out.push(CompatTool {
                    key: format!("custom:{name}"),
                    name: custom_tool_name(&dir),
                    dir,
                });
            }
        }
    }
    out
}

/// Pick the launch entry Steam would use for this runtime and branch.
pub fn select_launch_entry<'a>(
    app: &'a AppDetails,
    runtime: Runtime,
    branch: Option<&str>,
) -> Option<&'a LaunchEntry> {
    let os = runtime.target_os();
    let for_os: Vec<&LaunchEntry> = app
        .launch
        .iter()
        .filter(|l| match runtime {
            Runtime::Native => l.oslist.iter().any(|o| o == os),
            Runtime::Proton => l.oslist.is_empty() || l.oslist.iter().any(|o| o == os),
        })
        .collect();
    // `betakey` lists the branches an entry applies to, space separated (it may include
    // "public"); no betakey means every branch. When nothing matches the current branch
    // (EU5 tags every entry for old branches only), Steam still launches, so fall back to
    // all entries for the OS.
    let current = branch.unwrap_or("public");
    let for_branch: Vec<&LaunchEntry> = for_os
        .iter()
        .copied()
        .filter(|l| {
            l.beta_key
                .as_deref()
                .is_none_or(|keys| keys.split_whitespace().any(|k| k == current))
        })
        .collect();
    let mut candidates = if for_branch.is_empty() {
        for_os
    } else {
        for_branch
    };
    // Prefer: entries for the current beta, 64-bit, then the default type, in PICS order.
    candidates.sort_by_key(|l| {
        (
            l.beta_key.is_none(),
            l.osarch.as_deref() == Some("32"),
            !matches!(l.kind.as_deref(), None | Some("default" | "none")),
        )
    });
    candidates.first().copied()
}

/// Parsed Steam-style launch options: `ENV=1 wrapper %command% -args`.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct LaunchOptions {
    pub env: Vec<(String, String)>,
    pub wrappers: Vec<String>,
    pub args: Vec<String>,
}

impl LaunchOptions {
    pub fn parse(text: &str) -> Result<Self, LaunchError> {
        let words =
            shell_words::split(text).map_err(|e| LaunchError::LaunchOptions(e.to_string()))?;
        let mut out = Self::default();
        let Some(pos) = words.iter().position(|w| w == "%command%") else {
            out.args = words;
            return Ok(out);
        };
        for word in &words[..pos] {
            match word.split_once('=') {
                Some((k, v))
                    if !k.is_empty()
                        && out.wrappers.is_empty()
                        && k.chars().all(|c| c.is_ascii_alphanumeric() || c == '_') =>
                {
                    out.env.push((k.to_string(), v.to_string()));
                }
                _ => out.wrappers.push(word.clone()),
            }
        }
        out.args = words[pos + 1..].to_vec();
        Ok(out)
    }
}

/// A fully resolved command line.
#[derive(Clone, Debug, PartialEq)]
pub struct LaunchPlan {
    pub program: PathBuf,
    pub args: Vec<String>,
    pub env: Vec<(String, String)>,
    pub cwd: PathBuf,
}

impl LaunchPlan {
    pub fn command(&self) -> std::process::Command {
        let mut cmd = std::process::Command::new(&self.program);
        cmd.args(&self.args)
            .current_dir(&self.cwd)
            .envs(self.env.iter().map(|(k, v)| (k, v)));
        cmd
    }
}

/// Directory passed as `STEAM_COMPAT_CLIENT_INSTALL_PATH`. Proton copies optional
/// `legacycompat/steamclient*.dll` from here into the prefix for old DRM wrappers.
pub fn client_dir() -> PathBuf {
    config::data_dir().join("client")
}

/// Expand one tool's manifest command line (`/proton %verb%`) relative to its directory.
fn expand_tool(dir: &Path, manifest: &ToolManifest, verb: &str) -> Vec<String> {
    let line = manifest.commandline.replace("%verb%", verb);
    let mut words = shell_words::split(&line).unwrap_or_default();
    if let Some(first) = words.first_mut()
        && first.starts_with('/')
    {
        *first = format!("{}{}", dir.display(), first);
    }
    words
}

pub struct LaunchContext<'a> {
    pub settings: &'a Settings,
    pub app: &'a AppDetails,
    pub manifest: &'a AppManifest,
    pub library: &'a LibraryFolder,
    pub runtime: Runtime,
    /// Compatibility tool for `Runtime::Proton`.
    pub tool: Option<&'a CompatTool>,
    pub launch_options: &'a str,
}

pub fn plan(ctx: &LaunchContext) -> Result<LaunchPlan, LaunchError> {
    let app_id = ctx.app.app_id.to_string();
    let install = ctx.library.install_path(&ctx.manifest.install_dir);
    let entry = select_launch_entry(ctx.app, ctx.runtime, ctx.manifest.beta_key.as_deref())
        .ok_or_else(|| LaunchError::NoLaunchEntry(ctx.app.name.clone(), ctx.runtime.target_os()))?;
    let exe = install.join(entry.executable.replace('\\', "/"));
    if !exe.exists() {
        return Err(LaunchError::MissingExecutable(exe));
    }
    let cwd = match entry.working_dir.as_deref().filter(|w| !w.is_empty()) {
        Some(w) => install.join(w.replace('\\', "/")),
        None => install.clone(),
    };
    let mut game_args: Vec<String> = entry
        .arguments
        .as_deref()
        .map(|a| {
            shell_words::split(a)
                .unwrap_or_else(|_| a.split_whitespace().map(String::from).collect())
        })
        .unwrap_or_default();
    let options = LaunchOptions::parse(ctx.launch_options)?;
    game_args.extend(options.args.iter().cloned());

    let mut env = vec![
        ("SteamAppId".to_string(), app_id.clone()),
        ("SteamGameId".to_string(), app_id.clone()),
        ("SteamOverlayGameId".to_string(), app_id.clone()),
    ];

    let mut argv: Vec<String> = match ctx.runtime {
        Runtime::Native => {
            let mut v = vec![exe.display().to_string()];
            v.extend(game_args);
            v
        }
        Runtime::Proton => {
            let tool = ctx
                .tool
                .ok_or_else(|| LaunchError::ToolMissing("Proton".into()))?;
            let manifest = read_toolmanifest(&tool.dir)
                .ok_or_else(|| LaunchError::BadTool(tool.dir.clone()))?;
            let verb = "waitforexitandrun";
            let mut chain = Vec::new();
            let mut tool_paths = vec![tool.dir.display().to_string()];
            if let Some(required) = manifest.require_tool_appid {
                let dir = installed_app_dir(ctx.settings, required)
                    .ok_or(LaunchError::RequiredToolMissing(required))?;
                let req_manifest =
                    read_toolmanifest(&dir).ok_or_else(|| LaunchError::BadTool(dir.clone()))?;
                chain.extend(expand_tool(&dir, &req_manifest, verb));
                tool_paths.push(dir.display().to_string());
            }
            chain.extend(expand_tool(&tool.dir, &manifest, verb));
            chain.push(exe.display().to_string());
            chain.extend(game_args);

            let compat = ctx.library.compatdata(ctx.app.app_id);
            std::fs::create_dir_all(&compat)?;
            let client = client_dir();
            std::fs::create_dir_all(client.join("legacycompat"))?;
            let libraries: Vec<String> = ctx
                .settings
                .libraries
                .iter()
                .map(|l| l.steamapps().display().to_string())
                .collect();
            env.extend([
                ("STEAM_COMPAT_APP_ID".to_string(), app_id.clone()),
                (
                    "STEAM_COMPAT_DATA_PATH".to_string(),
                    compat.display().to_string(),
                ),
                (
                    "STEAM_COMPAT_CLIENT_INSTALL_PATH".to_string(),
                    client.display().to_string(),
                ),
                (
                    "STEAM_COMPAT_INSTALL_PATH".to_string(),
                    install.display().to_string(),
                ),
                (
                    "STEAM_COMPAT_LIBRARY_PATHS".to_string(),
                    libraries.join(":"),
                ),
                ("STEAM_COMPAT_TOOL_PATHS".to_string(), tool_paths.join(":")),
                (
                    "STEAM_COMPAT_SHADER_PATH".to_string(),
                    ctx.library
                        .steamapps()
                        .join("shadercache")
                        .join(&app_id)
                        .display()
                        .to_string(),
                ),
            ]);
            chain
        }
    };

    // Launch-option wrappers (gamemoderun, mangohud, ...) go in front of everything.
    if !options.wrappers.is_empty() {
        let mut wrapped = options.wrappers.clone();
        wrapped.append(&mut argv);
        argv = wrapped;
    }
    env.extend(options.env);
    let program = PathBuf::from(argv.remove(0));
    Ok(LaunchPlan {
        program,
        args: argv,
        env,
        cwd,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::pics::parse_app_buffer;

    fn app() -> AppDetails {
        let text = r#""appinfo" {
            "common" { "name" "Game" "type" "Game" }
            "config" { "installdir" "Game" "launch" {
                "0" { "executable" "bin\win64\game.exe" "arguments" "-dx12" "config" { "oslist" "windows" "osarch" "64" } }
                "1" { "executable" "bin/win32/game.exe" "config" { "oslist" "windows" "osarch" "32" } }
                "2" { "executable" "game.sh" "workingdir" "bin" "config" { "oslist" "linux" } }
                "3" { "executable" "game_beta.sh" "config" { "oslist" "linux" "betakey" "beta" } }
                "5" { "executable" "old.exe" "config" { "oslist" "windows" "osarch" "64" "betakey" "1.29 1.30" } }
                "4" { "executable" "tool.sh" "type" "option1" "config" { "oslist" "linux" } }
            } }
        }"#;
        AppDetails::from_kv(100, 1, &parse_app_buffer(text.as_bytes()).unwrap())
    }

    #[test]
    fn launch_entry_selection() {
        let a = app();
        assert_eq!(
            select_launch_entry(&a, Runtime::Proton, None)
                .unwrap()
                .executable,
            "bin\\win64\\game.exe"
        );
        assert_eq!(
            select_launch_entry(&a, Runtime::Native, None)
                .unwrap()
                .executable,
            "game.sh"
        );
        assert_eq!(
            select_launch_entry(&a, Runtime::Native, Some("beta"))
                .unwrap()
                .executable,
            "game_beta.sh"
        );
    }

    #[test]
    fn launch_options() {
        let o =
            LaunchOptions::parse("PROTON_LOG=1 DXVK_HUD=fps gamemoderun %command% -novid -w 1920")
                .unwrap();
        assert_eq!(
            o.env,
            vec![
                ("PROTON_LOG".into(), "1".into()),
                ("DXVK_HUD".into(), "fps".into())
            ]
        );
        assert_eq!(o.wrappers, vec!["gamemoderun".to_string()]);
        assert_eq!(o.args, vec!["-novid", "-w", "1920"]);
        let o = LaunchOptions::parse("-windowed").unwrap();
        assert_eq!(o.args, vec!["-windowed"]);
        assert!(LaunchOptions::parse("").unwrap().args.is_empty());
    }

    fn write(path: &Path, text: &str) {
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(path, text).unwrap();
    }

    #[test]
    fn proton_chain_and_native_plan() {
        let root = std::env::temp_dir().join(format!("steamqt-launch-{}", std::process::id()));
        let lib = LibraryFolder::new(root.join("lib"));
        lib.ensure_layout().unwrap();
        let mut settings = Settings::default();
        settings.add_library(&lib.path);

        // Game files and manifest.
        write(&lib.install_path("Game").join("bin/win64/game.exe"), "");
        write(&lib.install_path("Game").join("game.sh"), "");
        let manifest = AppManifest {
            app_id: 100,
            install_dir: "Game".into(),
            state_flags: 4,
            ..Default::default()
        };
        lib.write_manifest(&manifest).unwrap();

        // Sniper runtime and Proton installed as tool apps.
        for (id, dir, cmd, req) in [
            (
                SNIPER_APP_ID,
                "SteamLinuxRuntime_sniper",
                "/_v2-entry-point --verb=%verb% --",
                "",
            ),
            (
                PROTON_EXPERIMENTAL_APP_ID,
                "Proton - Experimental",
                "/proton %verb%",
                "\"require_tool_appid\" \"1628350\"",
            ),
        ] {
            write(
                &lib.install_path(dir).join("toolmanifest.vdf"),
                &format!("\"manifest\" {{ \"version\" \"2\" \"commandline\" \"{cmd}\" {req} }}"),
            );
            lib.write_manifest(&AppManifest {
                app_id: id,
                install_dir: dir.into(),
                state_flags: 4,
                ..Default::default()
            })
            .unwrap();
        }
        let tools = installed_tools(&settings);
        let proton = tools
            .iter()
            .find(|t| t.key == PROTON_EXPERIMENTAL_APP_ID.to_string())
            .expect("proton found");

        let a = app();
        let ctx = LaunchContext {
            settings: &settings,
            app: &a,
            manifest: &manifest,
            library: &lib,
            runtime: Runtime::Proton,
            tool: Some(proton),
            launch_options: "PROTON_LOG=1 %command% -skipintro",
        };
        let p = plan(&ctx).unwrap();
        let sniper = lib.install_path("SteamLinuxRuntime_sniper");
        assert_eq!(p.program, sniper.join("_v2-entry-point"));
        let proton_bin = lib
            .install_path("Proton - Experimental")
            .join("proton")
            .display()
            .to_string();
        let exe = lib
            .install_path("Game")
            .join("bin/win64/game.exe")
            .display()
            .to_string();
        assert_eq!(
            p.args,
            vec![
                "--verb=waitforexitandrun",
                "--",
                &proton_bin,
                "waitforexitandrun",
                &exe,
                "-dx12",
                "-skipintro"
            ]
        );
        let env = |k: &str| {
            p.env
                .iter()
                .find(|(key, _)| key == k)
                .map(|(_, v)| v.clone())
        };
        assert_eq!(
            env("STEAM_COMPAT_DATA_PATH"),
            Some(lib.compatdata(100).display().to_string())
        );
        assert_eq!(env("SteamAppId").as_deref(), Some("100"));
        assert_eq!(env("PROTON_LOG").as_deref(), Some("1"));
        assert!(lib.compatdata(100).is_dir());

        let native = LaunchContext {
            runtime: Runtime::Native,
            tool: None,
            launch_options: "",
            ..ctx
        };
        let p = plan(&native).unwrap();
        assert_eq!(p.program, lib.install_path("Game").join("game.sh"));
        assert_eq!(p.cwd, lib.install_path("Game").join("bin"));
        assert!(p.env.iter().all(|(k, _)| !k.starts_with("STEAM_COMPAT")));
        std::fs::remove_dir_all(root).unwrap();
    }
}
