use kinode_process_lib::kernel_types::{
    KernelCommand, KernelPrint, KernelPrintResponse, KernelResponse,
};
use kinode_process_lib::{call_init, eth, net, println, Address, Message, Request};
use std::collections::HashSet;

/// Fetching OS version from main package.. LMK if there's a better way...
const CARGO_TOML: &str = include_str!("../../../../Cargo.toml");

wit_bindgen::generate!({
    path: "target/wit",
    world: "process-v0",
});

call_init!(init);
fn init(our: Address) {
    // get identity
    let Ok(Ok(Message::Response { body, .. })) = Request::to(("our", "net", "distro", "sys"))
        .body(rmp_serde::to_vec(&net::NetAction::GetPeer(our.node.clone())).unwrap())
        .send_and_await_response(60)
    else {
        println!("failed to get response from net");
        return;
    };
    let Ok(net::NetResponse::Peer(Some(our_id))) = rmp_serde::from_slice(&body) else {
        println!("got malformed response from net");
        return;
    };

    // get eth providers
    let Ok(Message::Response { body, .. }) = Request::new()
        .target(("our", "eth", "distro", "sys"))
        .body(serde_json::to_vec(&eth::EthConfigAction::GetProviders).unwrap())
        .send_and_await_response(60)
        .unwrap()
    else {
        println!("failed to get response from eth");
        return;
    };
    let Ok(eth::EthConfigResponse::Providers(providers)) = serde_json::from_slice(&body) else {
        println!("failed to parse eth response");
        return;
    };

    // get eth subs
    let Ok(Message::Response { body, .. }) = Request::new()
        .target(("our", "eth", "distro", "sys"))
        .body(serde_json::to_vec(&eth::EthConfigAction::GetState).unwrap())
        .send_and_await_response(60)
        .unwrap()
    else {
        println!("failed to get response from eth");
        return;
    };
    let Ok(eth::EthConfigResponse::State {
        active_subscriptions,
        outstanding_requests,
    }) = serde_json::from_slice(&body)
    else {
        println!("failed to parse eth response");
        return;
    };

    // get number of processes
    let Ok(Message::Response { body, .. }) = Request::new()
        .target(("our", "kernel", "distro", "sys"))
        .body(serde_json::to_vec(&KernelCommand::Debug(KernelPrint::ProcessMap)).unwrap())
        .send_and_await_response(60)
        .unwrap()
    else {
        println!("failed to get response from kernel");
        return;
    };
    let Ok(KernelResponse::Debug(KernelPrintResponse::ProcessMap(map))) =
        serde_json::from_slice::<KernelResponse>(&body)
    else {
        println!("failed to parse kernel response");
        return;
    };
    let num_processes = map.len();
    print_bird(
        &our,
        our_id,
        providers,
        // sum up all the subscriptions
        active_subscriptions
            .values()
            .map(|v| v.len())
            .sum::<usize>(),
        outstanding_requests.len() as usize,
        num_processes,
    );
}

fn print_bird(
    our: &Address,
    our_id: net::Identity,
    providers: HashSet<eth::ProviderConfig>,
    active_subscriptions: usize,
    outstanding_requests: usize,
    num_processes: usize,
) {
    println!(
        r#"
    .`
`@@,,                     ,*   {}
  `@%@@@,            ,~-##`
    ~@@#@%#@@,      #####      Kinode {}
      ~-%######@@@, #####
         -%%#######@#####,     pubkey: {}
           ~^^%##########@     routing: {}
              >^#########@
                `>#######`     {} eth providers for chain IDs {}
               .>######%       {} active eth subscriptions
              /###%^#%         {} outstanding eth requests
            /##%@#  `
         ./######`
       /.^`.#^#^`
      `   ,#`#`#,              {} running processes
         ,/ /` `
       .*`
                   "#,
        our.node(),
        version_from_cargo_toml(),
        our_id.networking_key,
        routing_to_string(our_id.routing),
        providers.len(),
        providers
            .into_iter()
            .map(|p| p.chain_id.to_string())
            // remove duplicates
            .collect::<HashSet<_>>()
            .into_iter()
            .collect::<Vec<_>>()
            .join(", "),
        active_subscriptions,
        outstanding_requests,
        num_processes
    )
}

fn routing_to_string(routing: net::NodeRouting) -> String {
    match routing {
        net::NodeRouting::Direct { ip, ports } => format!(
            "direct at {} with {}",
            ip,
            ports.into_keys().into_iter().collect::<Vec<_>>().join(", ")
        ),
        net::NodeRouting::Routers(routers) => format!("{} routers", routers.len()),
    }
}

fn version_from_cargo_toml() -> String {
    let version = CARGO_TOML
        .lines()
        .find(|line| line.starts_with("version = "))
        .expect("Failed to find version in Cargo.toml");

    version
        .split('=')
        .last()
        .expect("Failed to parse version from Cargo.toml")
        .trim()
        .trim_matches('"')
        .to_string()
}
