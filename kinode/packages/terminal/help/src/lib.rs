use kinode_process_lib::{script, Address};

wit_bindgen::generate!({
    path: "target/wit",
    world: "process-v0",
});

const HELP_MESSAGES: [[&str; 2]; 11] = [
    ["alias", "\n\x1b[1malias\x1b[0m <shorthand> <process_id>: create an alias for a script.\n    - Example: \x1b[1malias get_block get_block:kns_indexer:sys\x1b[0m\n    - note: all of these listed commands are just default aliases for terminal scripts."],
    ["cat", "\n\x1b[1mcat\x1b[0m <vfs-file-path>: print the contents of a file in the terminal.\n    - Example: \x1b[1mcat /terminal:sys/pkg/scripts.json\x1b[0m"],
    ["echo", "\n\x1b[1mecho\x1b[0m <text>: print text to the terminal.\n    - Example: \x1b[1mecho foo\x1b[0m"],
    ["hi", "\n\x1b[1mhi\x1b[0m <name> <string>: send a text message to another node's command line.\n    - Example: \x1b[1mhi mothu.kino hello world\x1b[0m"],
    ["kfetch", "\n\x1b[1mkfetch\x1b[0m: print system information a la neofetch. No arguments."],
    ["kill", "\n\x1b[1mkill\x1b[0m <process-id>: terminate a running process. This will bypass any restart behavior–use judiciously.\n    - Example: \x1b[1mkill chess:chess:sys\x1b[0m"],
    ["m", "\n\x1b[1mm\x1b[0m <address> '<json>': send an inter-process message. <address> is formatted as <node>@<process_id>. <process_id> is formatted as <process_name>:<package_name>:<publisher_node>. JSON containing spaces must be wrapped in single-quotes (\x1b[1m''\x1b[0m).\n    - Example: \x1b[1mm our@eth:distro:sys \"SetPublic\" -a 5\x1b[0m\n    - the '-a' flag is used to expect a response with a given timeout\n    - \x1b[1mour\x1b[0m will always be interpolated by the system as your node's name"],
    ["net_diagnostics", "\n\x1b[1mnet_diagnostics\x1b[0m: print some useful networking diagnostic data."],
    ["peer", "\n\x1b[1mpeer\x1b[0m <name>: print the peer's PKI info, if it exists."],
    ["peers", "\n\x1b[1mpeers\x1b[0m: print the peers the node currently hold connections with."],
    ["top", "\n\x1b[1mtop\x1b[0m <process_id>: display kernel debugging info about a process. Leave the process ID blank to display info about all processes and get the total number of running processes.\n    - Example: \x1b[1mtop net:distro:sys\x1b[0m\n    - Example: \x1b[1mtop\x1b[0m"],
];

script!(init);
fn init(_our: Address, args: String) -> String {
    // if args is empty, print the entire help message.
    // if args contains the name of a command, print the help message for that command.
    // otherwise, print an error message.
    if args.is_empty() {
        let mut help_message = String::from(
            "\n====================\n\
            Kinode Terminal Help\n\
            ====================\n",
        );

        for [_, message] in HELP_MESSAGES.iter() {
            help_message.push_str(message);
            help_message.push_str("\n");
        }

        help_message.push_str(
            "For more help, look to the documentation at book.kinode.org.\n\
            ============================================================\n",
        );

        return help_message;
    } else if let Some(message) = HELP_MESSAGES.iter().find(|[cmd, _]| cmd == &args) {
        return message[1].to_string();
    } else {
        return format!("No help found for command \x1b[1m{args}\x1b[0m");
    }
}
