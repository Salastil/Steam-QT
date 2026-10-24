//! Live test: PICS app info cache (second fetch should be served from disk).
use std::time::Instant;
use steamqt_core::{catalog, session::Session};

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter("steamqt_core::catalog=debug")
        .init();
    let session = Session::anonymous().await?;
    let ids = [440, 570, 730, 1007, 1628350, 1493710];
    for pass in 1..=2 {
        let t = Instant::now();
        let apps = catalog::fetch_apps(&session, &ids).await?;
        println!(
            "pass {pass}: {} apps in {:?}: {:?}",
            apps.len(),
            t.elapsed(),
            apps.iter().map(|a| &a.name).collect::<Vec<_>>()
        );
    }
    Ok(())
}
