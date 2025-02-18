use hyperware_process_lib::{println, script, vfs, Address};

wit_bindgen::generate!({
    path: "target/wit",
    world: "process-v1",
});

const USAGE: &str = "\x1b[1mUsage:\x1b[0m cat <file_path>";

script!(init);
fn init(_our: Address, args: String) -> String {
    if args.is_empty() {
        return format!("Print the contents of a file to the terminal.\n{USAGE}");
    }

    match vfs::File::new(&args, 5).read() {
        Ok(data) => String::from_utf8_lossy(&data).to_string(),
        Err(_) => format!("failed to read file {args} from VFS.\n{USAGE}"),
    }
}
