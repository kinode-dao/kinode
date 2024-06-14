const KINODE_WIT_0_7_0_URL: &str =
    "https://raw.githubusercontent.com/kinode-dao/kinode-wit/aa2c8b11c9171b949d1991c32f58591c0e881f85/kinode.wit";

const KINODE_WIT_0_8_0_URL: &str =
    "https://raw.githubusercontent.com/kinode-dao/kinode-wit/v0.8/kinode.wit";

fn main() {
    if std::env::var("SKIP_BUILD_SCRIPT").is_ok() {
        println!("Skipping build script");
        return;
    }

    let pwd = std::env::current_dir().expect("Failed to get current directory");

    // let wit_file = pwd.join("wit-v0.7.0").join("kinode.wit");

    // let rt = tokio::runtime::Runtime::new().unwrap();
    // rt.block_on(async {
    //     kit::build::download_file(KINODE_WIT_0_7_0_URL, &wit_file)
    //         .await
    //         .expect("Failed to download WIT 0.7");
    // });

    // let wit_file = pwd.join("wit-v0.8.0").join("kinode.wit");

    // let rt = tokio::runtime::Runtime::new().unwrap();
    // rt.block_on(async {
    //     kit::build::download_file(KINODE_WIT_0_8_0_URL, &wit_file)
    //         .await
    //         .expect("Failed to download WIT 0.8");
    // })
}
