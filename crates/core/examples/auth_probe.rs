//! Live check of the credentials auth path with a wrong password (expects InvalidPassword).
#[tokio::main]
async fn main() {
    match steamqt_core::session::authenticate("steamqt_probe_account", "definitely-wrong-password")
        .await
    {
        Ok(_) => println!("unexpected: auth step returned"),
        Err(e) => println!("error (expected): {e}"),
    }
}
