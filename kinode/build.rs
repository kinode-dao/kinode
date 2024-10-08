use std::path::PathBuf;

const CANONICAL_PACKAGES_ZIP_PATH: &str = "../target/packages.zip";

fn main() -> anyhow::Result<()> {
    let path_to_packages_zip = match std::env::var("PATH_TO_PACKAGES_ZIP") {
        Err(_) => {
            let build_package_script_path = PathBuf::from("../scripts/build_package");
            let mut child = std::process::Command::new("cargo")
                .arg("run")
                .current_dir(&build_package_script_path)
                .spawn()?;
            let result = child.wait()?;
            if !result.success() {
                return Err(anyhow::anyhow!("Failed to build packages."));
            }
            CANONICAL_PACKAGES_ZIP_PATH.to_string()
        }
        Ok(env_var) => env_var,
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
