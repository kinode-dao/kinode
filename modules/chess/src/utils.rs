use crate::*;

pub fn save_chess_state(state: &ChessState) {
    let stored_state = convert_state(&state);
    set_state(&bincode::serialize(&stored_state).unwrap());
}

fn convert_game(game: Game) -> StoredGame {
    StoredGame {
        id: game.id,
        turns: game.turns,
        board: game.board.fen(),
        white: game.white,
        black: game.black,
        ended: game.ended,
    }
}

fn convert_state(state: &ChessState) -> StoredChessState {
    StoredChessState {
        games: state
            .games
            .iter()
            .map(|(id, game)| (id.to_string(), convert_game(game.clone())))
            .collect(),
        records: state.records.clone(),
    }
}

pub fn json_game(game: &Game) -> serde_json::Value {
    serde_json::json!({
        "id": game.id,
        "turns": game.turns,
        "board": game.board.fen(),
        "white": game.white,
        "black": game.black,
        "ended": game.ended,
    })
}

pub fn send_ws_update(our: &Address, game: &Game) -> anyhow::Result<()> {
    Request::new()
        .target((&our.node, "http_server", "sys", "uqbar"))
        .ipc(
            serde_json::json!({
                "EncryptAndForward": {
                    "channel_id": our.process.to_string(),
                    "forward_to": {
                        "node": our.node.clone(),
                        "process": {
                            "process_name": "http_server",
                            "package_name": "sys",
                            "publisher_node": "uqbar"
                        }
                    }, // node, process
                    "json": Some(serde_json::json!({ // this is the JSON to forward
                        "WebSocketPush": {
                            "target": {
                                "node": our.node.clone(),
                                "id": "chess", // If the message passed in an ID then we could send to just that ID
                            }
                        }
                    })),
                }

            })
            .to_string()
            .as_bytes()
            .to_vec(),
        )
        .payload(Payload {
            mime: Some("application/json".to_string()),
            bytes: serde_json::json!({
                "kind": "game_update",
                "data": json_game(game),
            })
            .to_string()
            .as_bytes()
            .to_vec(),
        })
        .send()
}
