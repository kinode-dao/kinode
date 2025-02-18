use std::fs;
use std::path::Path;

use sha2::{Digest, Sha256};

const KIT_CACHE: &str = "/tmp/hyperware-kit-cache";
const KINODE_WIT_1_0_0_URL: &str =
    //"https://raw.githubusercontent.com/hyperware-ai/hyperware-wit/v1.0.0/hyperware.wit";
    "https://gist.githubusercontent.com/nick1udwig/3cfef4c96d945513c5fbc69d6bfbb4d9/raw/46d9a404813009a2adab54e9cc3e950cbe14ba3f/hyperware.wit";

/// copied from `kit`
async fn download_file(url: &str, path: &Path) -> anyhow::Result<()> {
    fs::create_dir_all(&KIT_CACHE)?;
    let mut hasher = Sha256::new();
    hasher.update(url.as_bytes());
    let hashed_url = hasher.finalize();
    let hashed_url_path = Path::new(KIT_CACHE).join(format!("{hashed_url:x}"));

    let content = if hashed_url_path.exists() {
        fs::read(hashed_url_path)?
    } else {
        let response = reqwest::get(url).await?;

        // Check if response status is 200 (OK)
        if response.status() != reqwest::StatusCode::OK {
            return Err(anyhow::anyhow!(
                "Failed to download file: HTTP Status {}",
                response.status()
            ));
        }

        let content = response.bytes().await?.to_vec();
        fs::write(hashed_url_path, &content)?;
        content
    };

    if path.exists() {
        if path.is_dir() {
            fs::remove_dir_all(path)?;
        } else {
            let existing_content = fs::read(path)?;
            if content == existing_content {
                return Ok(());
            }
        }
    }
    fs::create_dir_all(
        path.parent()
            .ok_or_else(|| anyhow::anyhow!("path doesn't have parent"))?,
    )?;
    fs::write(path, &content)?;
    Ok(())
}

fn main() {
    if std::env::var("SKIP_BUILD_SCRIPT").is_ok() {
        println!("Skipping build script");
        return;
    }

    let pwd = std::env::current_dir().expect("Failed to get current directory");

    let wit_file = pwd.join("wit-v1.0.0").join("hyperware.wit");

    let rt = tokio::runtime::Runtime::new().unwrap();
    rt.block_on(async {
        download_file(KINODE_WIT_1_0_0_URL, &wit_file)
            .await
            .expect("Failed to download WIT 1.0");
    });
}
