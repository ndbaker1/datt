pub mod gogoanime;
pub mod megacloud;

async fn fetch(url: &str) -> crate::Res<Vec<u8>> {
    Ok(reqwest::get(url)
        .await?
        .error_for_status()?
        .bytes()
        .await?
        .to_vec())
}
