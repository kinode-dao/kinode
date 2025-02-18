use std::io::Read;
use std::path::{Path, PathBuf};

use sha2::Digest;

const CANONICAL_PACKAGES_ZIP_PATH: &str = "../target/packages.zip";

macro_rules! p {
    ($($tokens: tt)*) => {
        println!("cargo:warning={}", format!($($tokens)*))
    }
}

fn compute_hash(file_path: &Path) -> anyhow::Result<String> {
    let input_file = std::fs::File::open(file_path)?;
    let mut reader = std::io::BufReader::new(input_file);
    let mut hasher = sha2::Sha256::new();
    let mut buffer = [0; 1024]; // buffer for chunks of the file

    loop {
        let count = reader.read(&mut buffer)?;
        if count == 0 {
            break;
        }
        hasher.update(&buffer[..count]);
    }

    Ok(format!("{:x}", hasher.finalize()))
}

fn main() -> anyhow::Result<()> {
    let path_to_packages_zip = match std::env::var("PATH_TO_PACKAGES_ZIP") {
        Ok(env_var) => {
            let path = PathBuf::from(&env_var);
            if !path.exists() {
                let path = std::env::current_dir()?;
                let Some(path) = path.parent() else {
                    return Err(anyhow::anyhow!(
                        "Given path to packages {env_var} not found (cwd: {:?})",
                        std::env::current_dir()
                    ));
                };
                let path = path.join(&env_var);
                if path.exists() {
                    path.display().to_string()
                } else {
                    return Err(anyhow::anyhow!(
                        "Given path to packages {env_var} not found in parent of cwd: {:?}",
                        std::env::current_dir()
                    ));
                }
            } else {
                env_var
            }
        }
        Err(_) => {
            let canonical_path = PathBuf::from(CANONICAL_PACKAGES_ZIP_PATH);
            if canonical_path.exists() {
                p!("No path given via PATH_TO_PACKAGES_ZIP envvar. Defaulting to path of `hyperdrive/target/packages.zip`.");
                CANONICAL_PACKAGES_ZIP_PATH.to_string()
            } else {
                return Err(anyhow::anyhow!("You must build packages.zip with scripts/build-packages or set PATH_TO_PACKAGES_ZIP to point to your desired pacakges.zip (default path at hyperdrive/target/packages.zip was not populated)."));
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
    let canonical_packages_zip_path = PathBuf::from(CANONICAL_PACKAGES_ZIP_PATH);
    if !canonical_packages_zip_path.exists() {
        std::fs::File::create(&canonical_packages_zip_path)?;
    }
    let canonical_packages_zip_path = PathBuf::from(CANONICAL_PACKAGES_ZIP_PATH).canonicalize()?;
    if path_to_packages_zip_path != canonical_packages_zip_path {
        std::fs::copy(&path_to_packages_zip_path, &canonical_packages_zip_path)?;
    }

    if !std::env::var("SKIP_BUILD_FRONTEND").is_ok() {
        // build core frontends
        let pwd = std::env::current_dir()?;
        let core_frontends = vec!["src/register-ui"];

        // for each frontend, execute build.sh
        for frontend in core_frontends {
            let status = std::process::Command::new("sh")
                .current_dir(pwd.join(frontend))
                .arg("./build.sh")
                .status()?;
            if !status.success() {
                return Err(anyhow::anyhow!("Failed to build frontend: {}", frontend));
            }
        }
    }

    let version = if let Ok(version) = std::env::var("DOCKER_BUILD_IMAGE_VERSION") {
        // embed the DOCKER_BUILD_IMAGE_VERSION
        version
    } else {
        "none".to_string()
    };
    println!("cargo:rustc-env=DOCKER_BUILD_IMAGE_VERSION={version}");

    let packages_zip_hash = compute_hash(&canonical_packages_zip_path)?;
    println!("cargo:rustc-env=PACKAGES_ZIP_HASH={packages_zip_hash}");

    Ok(())
}
