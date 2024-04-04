const KINODE_WIT_URL: &str =
    "https://raw.githubusercontent.com/kinode-dao/kinode-wit/aa2c8b11c9171b949d1991c32f58591c0e881f85/kinode.wit";

fn main() -> anyhow::Result<()> {
    if std::env::var("SKIP_BUILD_SCRIPT").is_ok() {
        println!("Skipping build script");
        return Ok(());
    }

    let pwd = std::env::current_dir()?;

    let wit_file = pwd
        .join("wit")
        .join("kinode.wit");

    let rt = tokio::runtime::Runtime::new().unwrap();
    rt.block_on(async {
        kit::build::download_file(KINODE_WIT_URL, &wit_file)
            .await
            .map_err(|e| anyhow::anyhow!("{:?}", e))?;
        Ok(())
    })
}
