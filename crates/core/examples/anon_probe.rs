//! Live smoke test: anonymous session, licences, PICS, CDN directory.
use steamqt_core::session::Session;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    tracing_subscriber::fmt().with_env_filter("info").init();
    let session = Session::anonymous().await?;
    println!("steam_id={} cell={}", session.steam_id(), session.cell_id());
    let licenses = session.licenses().await;
    println!(
        "licences: {:?}",
        licenses.iter().map(|l| l.package_id).collect::<Vec<_>>()
    );
    let pkgs: Vec<_> = licenses
        .iter()
        .map(|l| (l.package_id, l.access_token))
        .collect();
    for p in session.packages(&pkgs).await? {
        println!(
            "package {}: {} apps, {} depots",
            p.package_id,
            p.app_ids.len(),
            p.depot_ids.len()
        );
    }
    for app in session.apps(&[440, 1628350]).await? {
        println!(
            "app {} '{}' {:?} linux={} depots={} branches={:?}",
            app.app_id,
            app.name,
            app.kind,
            app.supports_linux(),
            app.depots.len(),
            app.branches
                .iter()
                .map(|b| (&b.name, b.build_id))
                .collect::<Vec<_>>()
        );
    }
    let cdn = session.cdn_servers().await?;
    println!(
        "cdn: {:?}",
        cdn.iter()
            .take(3)
            .map(|s| (&s.host, s.port, s.https))
            .collect::<Vec<_>>()
    );
    let now = session.changes_since(0).await?;
    println!("change number now: {}", now.current);
    let recent = session
        .changes_since(now.current.saturating_sub(200))
        .await?;
    println!(
        "apps changed in last 200 changes: {:?}",
        recent.apps.map(|a| a.len())
    );
    Ok(())
}
