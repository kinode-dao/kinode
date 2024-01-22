use kinode_process_lib::{
    Address,
    await_message,
    println
};

wit_bindgen::generate!({
    path: "../../../wit",
    world: "process",
    exports: {
        world: Component,
    },
});

struct Component;
impl Guest for Component {
    fn init(our: String) {


        let our: Address = our.parse().unwrap();

        match main(our) {
            Ok(_) => {}
            Err(e) => {
                println!(": error: {:?}", e);
            }
        }
    }
}

fn main(our: Address) -> anyhow::Result<()> {

    loop {
        match await_message() {
            Err(e) => {
                continue;
            }
            Ok(message) =>  {
                println!("message");
            }
        }
    };
    Ok(())
}
