use std::{fs, io::copy};

fn main() {
    if std::env::var("SKIP_BUILD_SCRIPT").is_ok() {
        println!("Skipping build script");
        return;
    }

    let pwd = std::env::current_dir().unwrap();

    // Pull wit from git repo
    let wit_dir = pwd.join("wit");
    fs::create_dir_all(&wit_dir).unwrap();
    let wit_file = wit_dir.join("kinode.wit");
    if !wit_file.exists() {
        // TODO: cache in better way
        let mut wit_file = fs::File::create(&wit_file).unwrap();
        let kinode_wit_url =
            "https://raw.githubusercontent.com/uqbar-dao/kinode-wit/master/kinode.wit";
        let mut response = reqwest::blocking::get(kinode_wit_url).unwrap();
        copy(&mut response, &mut wit_file).unwrap();
    }
}
