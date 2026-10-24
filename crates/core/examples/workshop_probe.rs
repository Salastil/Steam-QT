//! Live check: workshop item details.
use steamqt_core::session::Session;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let session = Session::anonymous().await?;
    for item in session.workshop_details(&[160250458]).await? {
        println!("{item:?}");
    }
    let app = session.apps(&[4000]).await?.pop().unwrap();
    println!("gmod workshop depot: {:?}", app.workshop_depot);
    Ok(())
}
