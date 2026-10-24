//! Debug: which launch entry each runtime would use for the given apps (live PICS).
use steamqt_core::{install::Runtime, launch, session::Session};
#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let ids: Vec<u32> = std::env::args()
        .skip(1)
        .filter_map(|a| a.parse().ok())
        .collect();
    let session = Session::anonymous().await?;
    for app in session.apps(&ids).await? {
        let pick = |rt| launch::select_launch_entry(&app, rt, None).map(|l| l.executable.clone());
        if std::env::var("DUMP").is_ok() {
            println!("{:#?}", app.launch);
        }
        println!(
            "{} {:<32} native={:?} proton={:?} ({} entries)",
            app.app_id,
            app.name,
            pick(Runtime::Native),
            pick(Runtime::Proton),
            app.launch.len()
        );
    }
    Ok(())
}
