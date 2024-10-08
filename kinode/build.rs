use std::path::PathBuf;

const CANONICAL_PACKAGES_ZIP_PATH: &str = "../target/packages.zip";

macro_rules! p {
    ($($tokens: tt)*) => {
        println!("cargo:warning={}", format!($($tokens)*))
    }
}

fn main() -> anyhow::Result<()> {
    let path_to_packages_zip = match std::env::var("PATH_TO_PACKAGES_ZIP") {
        Ok(env_var) => env_var,
        Err(_) => {
            let canonical_path = PathBuf::from(CANONICAL_PACKAGES_ZIP_PATH);
            if canonical_path.exists() {
                p!("No path given via PATH_TO_PACKAGES_ZIP envvar. Defaulting to path of `kinode/target/packages.zip`.");
                CANONICAL_PACKAGES_ZIP_PATH.to_string()
            } else {
                return Err(anyhow::anyhow!("You must build packages.zip with scripts/build_packages or set PATH_TO_PACKAGES_ZIP to point to your desired pacakges.zip (default path at kinode/target/packages.zip was not populated)."));
            }
        }
    };
    let path = PathBuf::from(&path_to_packages_zip);
    if !path.exists() {
        return Err(anyhow::anyhow!(
            "Path to packages {path_to_packages_zip} does not exist."
        ));
    }

    let path_to_packages_zip_path = PathBuf::from(&path_to_packages_zip).canonicalize()?;
    let canonical_packages_zip_path = PathBuf::from(CANONICAL_PACKAGES_ZIP_PATH).canonicalize()?;
    if path_to_packages_zip_path != canonical_packages_zip_path {
        std::fs::copy(path_to_packages_zip_path, CANONICAL_PACKAGES_ZIP_PATH)?;
    }

    let version = if let Ok(version) = std::env::var("DOCKER_BUILD_IMAGE_VERSION") {
        // embed the DOCKER_BUILD_IMAGE_VERSION
        version
    } else {
        "none".to_string()
    };
    println!("cargo:rustc-env=DOCKER_BUILD_IMAGE_VERSION={version}");

    Ok(())
}
