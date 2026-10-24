//! Live test: anonymously install the Steamworks SDK Redist (app 1007) into a scratch library.
use std::collections::HashSet;
use steamqt_core::install::{self, InstallRequest, Runtime};
use steamqt_core::library::LibraryFolder;
use steamqt_core::session::Session;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter("info,steamqt_core=debug")
        .init();
    let lib_path = std::env::args().nth(1).expect("library path");
    let session = Session::anonymous().await?;
    let app = session.apps(&[1007]).await?.pop().expect("app 1007");
    println!(
        "{} depots={:?}",
        app.name,
        app.depots
            .iter()
            .map(|d| (d.id, d.oslist.clone()))
            .collect::<Vec<_>>()
    );
    let mut req = InstallRequest {
        app: app.clone(),
        library: LibraryFolder::new(&lib_path),
        runtime: Runtime::Native,
        branch: "public".into(),
        language: "english".into(),
        owned: HashSet::new(),
        disabled_dlc: vec![],
        verify: false,
    };
    for pass in 1..=3 {
        if pass == 3 {
            // Switch the installed app to the Windows build.
            req.runtime = Runtime::Proton;
        }
        let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel();
        let printer = tokio::spawn(async move {
            while let Some(p) = rx.recv().await {
                println!("  {p:?}");
            }
        });
        let m = install::install(&session, &req, tx, Default::default()).await?;
        printer.await?;
        println!(
            "pass {pass}: state={} build={} depots={:?} size={}",
            m.state_flags, m.build_id, m.installed_depots, m.size_on_disk
        );
        println!("needs_update={}", install::needs_update(&app, &m));
    }
    Ok(())
}
