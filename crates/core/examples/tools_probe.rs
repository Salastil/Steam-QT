//! Live check of compatibility-tool app ids and their PICS data.
use steamqt_core::session::Session;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let session = Session::anonymous().await?;
    let ids = [
        1493710, 2805730, 3658110, 2348590, 1887720, 2180100, 1628350, 1391110, 1070560, 4183110,
        3889360,
    ];
    for app in session.apps(&ids).await? {
        let depots: Vec<_> = app
            .depots
            .iter()
            .map(|d| {
                (
                    d.id,
                    d.oslist.clone(),
                    d.manifests.get("public").map(|m| m.size),
                )
            })
            .collect();
        println!(
            "{} {:?} '{}' installdir={:?} depots={:?}",
            app.app_id, app.kind, app.name, app.install_dir, depots
        );
    }
    Ok(())
}
