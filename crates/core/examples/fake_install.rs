//! Test helper: record an app as installed (matching current PICS manifests) without
//! downloading it, with a stub launcher. Usage: fake_install <library> <appid>
use std::collections::HashSet;
use steamqt_core::install::{self, InstallRequest, Runtime};
use steamqt_core::launch;
use steamqt_core::library::{AppManifest, InstalledDepot, LibraryFolder, state};
use steamqt_core::session::Session;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let mut args = std::env::args().skip(1);
    let lib = LibraryFolder::new(args.next().expect("library"));
    let app_id: u32 = args.next().expect("appid").parse()?;
    let session = Session::anonymous().await?;
    let app = session.apps(&[app_id]).await?.pop().expect("app");
    let req = InstallRequest {
        app: app.clone(),
        library: lib.clone(),
        runtime: Runtime::Native,
        branch: "public".into(),
        language: "english".into(),
        owned: HashSet::new(),
        disabled_dlc: vec![],
        verify: false,
    };
    let planned = install::plan(&req)?;
    let mut m = AppManifest {
        app_id,
        name: app.name.clone(),
        state_flags: state::FULLY_INSTALLED,
        install_dir: app.install_dir.clone().unwrap(),
        size_on_disk: 1,
        runtime: Some("native".into()),
        build_id: app.branch("public").map(|b| b.build_id).unwrap_or(0),
        ..Default::default()
    };
    for p in planned {
        m.installed_depots.insert(
            p.depot_id,
            InstalledDepot {
                manifest: p.manifest.gid,
                size: p.manifest.size,
                dlc_app_id: None,
            },
        );
    }
    lib.ensure_layout()?;
    lib.write_manifest(&m)?;
    let entry =
        launch::select_launch_entry(&app, Runtime::Native, None).expect("linux launch entry");
    let exe = lib
        .install_path(&m.install_dir)
        .join(entry.executable.replace('\\', "/"));
    std::fs::create_dir_all(exe.parent().unwrap())?;
    std::fs::write(
        &exe,
        "#!/bin/sh\necho \"started appid=$SteamAppId cwd=$(pwd) args=$*\" > \"$HOME/launch-marker\"\nsleep 30\n",
    )?;
    use std::os::unix::fs::PermissionsExt;
    std::fs::set_permissions(&exe, std::fs::Permissions::from_mode(0o755))?;
    println!("installed stub {} at {}", app.name, exe.display());
    Ok(())
}
