use crate::*;

pub fn save_chess_state(state: &ChessState) {
    set_state(&bincode::serialize(&state.games).unwrap());
}

pub fn load_chess_state() -> ChessState {
    match get_typed_state(|bytes| Ok(bincode::deserialize::<HashMap<String, Game>>(bytes)?)) {
        Some(games) => ChessState {
            games,
            clients: HashSet::new(),
        },
        None => ChessState {
            games: HashMap::new(),
            clients: HashSet::new(),
        },
    }
}

pub fn send_ws_update(
    our: &Address,
    game: &Game,
    open_channels: &HashSet<u32>,
) -> anyhow::Result<()> {
    for channel in open_channels {
        Request::new()
            .target((&our.node, "http_server", "sys", "uqbar"))
            .ipc(serde_json::to_vec(
                &http::HttpServerAction::WebSocketPush {
                    channel_id: *channel,
                    message_type: http::WsMessageType::Binary,
                },
            )?)
            .payload(Payload {
                mime: Some("application/json".to_string()),
                bytes: serde_json::json!({
                    "kind": "game_update",
                    "data": game,
                }).to_string().into_bytes(),
            })
            .send()?;
    }
    Ok(())
}
