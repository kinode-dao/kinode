cargo_component_bindings::generate!();

use bindings::component::uq_process::types::*;
use bindings::{
    get_payload, print_to_terminal, receive, send_and_await_response, send_request, send_requests,
    send_response, Guest,
};
use serde::{Deserialize, Serialize};
use serde_json::json;
use std::collections::HashMap;
extern crate base64;
extern crate pleco;
use pleco::Board;

#[allow(dead_code)]
mod process_lib;

struct Component;

#[derive(Clone, Debug)]
struct Game {
    pub id: String, // the node with whom we are playing
    pub turns: u64,
    pub board: Board,
    pub white: String,
    pub black: String,
    pub ended: bool,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
struct StoredGame {
    pub id: String, // the node with whom we are playing
    pub turns: u64,
    pub board: String,
    pub white: String,
    pub black: String,
    pub ended: bool,
}

#[derive(Clone, Debug)]
struct ChessState {
    pub games: HashMap<String, Game>, // game is by opposing player id
    pub records: HashMap<String, (u64, u64, u64)>, // wins, losses, draws
}

#[derive(Clone, Debug, Serialize, Deserialize)]
struct StoredChessState {
    pub games: HashMap<String, StoredGame>, // game is by opposing player id
    pub records: HashMap<String, (u64, u64, u64)>, // wins, losses, draws
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

fn convert_state(state: ChessState) -> StoredChessState {
    StoredChessState {
        games: state
            .games
            .iter()
            .map(|(id, game)| (id.to_string(), convert_game(game.clone())))
            .collect(),
        records: state.records.clone(),
    }
}

fn json_game(game: &Game) -> serde_json::Value {
    serde_json::json!({
        "id": game.id,
        "turns": game.turns,
        "board": game.board.fen(),
        "white": game.white,
        "black": game.black,
        "ended": game.ended,
    })
}

fn send_http_response(status: u16, headers: HashMap<String, String>, payload_bytes: Vec<u8>) {
    send_response(
        &Response {
            inherit: false,
            ipc: Some(
                serde_json::json!({
                    "status": status,
                    "headers": headers,
                })
                .to_string(),
            ),
            metadata: None,
        },
        Some(&Payload {
            mime: Some("application/octet-stream".to_string()),
            bytes: payload_bytes,
        }),
    )
}

fn send_ws_update(our_name: String, game: Game) {
    send_request(
        &Address {
            node: our_name.clone(),
            process: ProcessId::from_str("encryptor:sys:uqbar").unwrap(),
        },
        &Request {
            inherit: false,
            expects_response: None,
            ipc: Some(
                serde_json::json!({
                    "EncryptAndForwardAction": {
                        "channel_id": "chess",
                        "forward_to": {
                            "node": our_name.clone(),
                            "process": {
                                "Name": "http_server"
                            }, // If the message passed in an ID then we could send to just that ID
                        }, // node, process
                        "json": Some(serde_json::json!({ // this is the JSON to forward
                            "WebSocketPush": {
                                "target": {
                                    "node": our_name.clone(),
                                    "id": "chess", // If the message passed in an ID then we could send to just that ID
                                }
                            }
                        })),
                    }

                })
                .to_string(),
            ),
            metadata: None,
        },
        None,
        Some(&Payload {
            mime: Some("application/json".to_string()),
            bytes: serde_json::json!({
                "kind": "game_update",
                "data": json_game(&game),
            })
            .to_string()
            .as_bytes()
            .to_vec(),
        }),
    );
}

fn response_success() -> bool {
    let Some(payload) = get_payload() else {
        return false;
    };

    let Ok(status) = String::from_utf8(payload.bytes) else {
        return false;
    };

    status == "success"
}

fn save_chess_state(state: ChessState) {
    let stored_state = convert_state(state);
    process_lib::set_state::<StoredChessState>(&stored_state);
}

const CHESS_PAGE: &str = include_str!("chess.html");
const CHESS_JS: &str = include_str!("index.js");
const CHESS_CSS: &str = include_str!("index.css");

impl Guest for Component {
    fn init(our: Address) {
        print_to_terminal(0, "CHESS: start");

        let bindings_address = Address {
            node: our.node.clone(),
            process: ProcessId::from_str("http_server:sys:uqbar").unwrap(),
        };

        // <address, request, option<context>, option<payload>>
        let http_endpoint_binding_requests: [(Address, Request, Option<Context>, Option<Payload>);
            2] = [
            (
                bindings_address.clone(),
                Request {
                    inherit: false,
                    expects_response: None,
                    ipc: Some(json!({
                        "BindPath": {
                            "path": "/",
                            "authenticated": true,
                            "local_only": false
                        }
                    }).to_string()),
                    metadata: None,
                },
                None,
                None,
            ),
            (
                bindings_address.clone(),
                Request {
                    inherit: false,
                    expects_response: None,
                    ipc: Some(json!({
                        "BindPath": {
                            "path": "/games",
                            "authenticated": true,
                            "local_only": false
                        }
                    }).to_string()),
                    metadata: None,
                },
                None,
                None,
            ),
        ];
        send_requests(&http_endpoint_binding_requests);

        let mut state: ChessState = match process_lib::get_state::<StoredChessState>() {
            Some(state) => {
                let mut games = HashMap::new();
                for (id, game) in state.games {
                    if let Ok(board) = Board::from_fen(&game.board) {
                        games.insert(
                            id,
                            Game {
                                id: game.id.clone(),
                                turns: game.turns,
                                board: board,
                                white: game.white.clone(),
                                black: game.black.clone(),
                                ended: game.ended,
                            },
                        );
                    } else {
                        games.insert(
                            id,
                            Game {
                                id: game.id.clone(),
                                turns: 0,
                                board: Board::start_pos(),
                                white: game.white.clone(),
                                black: game.black.clone(),
                                ended: game.ended,
                            },
                        );
                    }
                }

                ChessState {
                    games,
                    records: state.records,
                }
            }
            None => ChessState {
                games: HashMap::new(),
                records: HashMap::new(),
            },
        };

        loop {
            let Ok((source, message)) = receive() else {
                print_to_terminal(0, "chess: got network error");
                continue;
            };
            let Message::Request(request) = message else {
                print_to_terminal(1, "chess: got unexpected Response");
                continue;
            };

            if let Some(json) = request.ipc {
                print_to_terminal(1, format!("chess: JSON {}", json).as_str());
                let message_json: serde_json::Value = match serde_json::from_str(&json) {
                    Ok(v) => v,
                    Err(_) => {
                        print_to_terminal(1, "chess: failed to parse ipc JSON, skipping");
                        continue;
                    }
                };

                print_to_terminal(1, "chess: parsed ipc JSON");

                if source.process.to_string() == "chess:chess:uqbar" {
                    let action = message_json["action"].as_str().unwrap_or("");
                    let game_id = source.node.clone();

                    match action {
                        "new_game" => {
                            // make a new game with source.node if the current game has ended
                            if let Some(game) = state.games.get(&game_id) {
                                if !game.ended {
                                    send_response(
                                        &Response {
                                            inherit: false,
                                            ipc: None,
                                            metadata: None,
                                        },
                                        Some(&Payload {
                                            mime: Some("application/octet-stream".to_string()),
                                            bytes: "conflict".as_bytes().to_vec(),
                                        }),
                                    );
                                    continue;
                                }
                            }
                            let game = Game {
                                id: game_id.clone(),
                                turns: 0,
                                board: Board::start_pos(),
                                white: message_json["white"]
                                    .as_str()
                                    .unwrap_or(game_id.as_str())
                                    .to_string(),
                                black: message_json["black"]
                                    .as_str()
                                    .unwrap_or(our.node.as_str())
                                    .to_string(),
                                ended: false,
                            };
                            state.games.insert(game_id.clone(), game.clone());

                            send_ws_update(our.node.clone(), game.clone());

                            save_chess_state(state.clone());

                            send_response(
                                &Response {
                                    inherit: false,
                                    ipc: None,
                                    metadata: None,
                                },
                                Some(&Payload {
                                    mime: Some("application/octet-stream".to_string()),
                                    bytes: "success".as_bytes().to_vec(),
                                }),
                            );
                            continue;
                        }
                        "make_move" => {
                            // check the move and then update if correct and send WS update
                            let Some(game) = state.games.get_mut(&game_id) else {
                                send_response(
                                    &Response {
                                        inherit: false,
                                        ipc: None,
                                        metadata: None,
                                    },
                                    Some(&Payload {
                                        mime: Some("application/octet-stream".to_string()),
                                        bytes: "not found".as_bytes().to_vec(),
                                    }),
                                );
                                continue;
                            };
                            let valid_move = game
                                .board
                                .apply_uci_move(message_json["move"].as_str().unwrap_or(""));
                            if valid_move {
                                game.turns += 1;
                                let checkmate = game.board.checkmate();
                                let draw = game.board.stalemate();

                                if checkmate || draw {
                                    game.ended = true;
                                    let winner = if checkmate {
                                        if game.turns % 2 == 1 {
                                            game.white.clone()
                                        } else {
                                            game.black.clone()
                                        }
                                    } else {
                                        "".to_string()
                                    };

                                    // update the records
                                    if draw {
                                        if let Some(record) = state.records.get_mut(&game.id) {
                                            record.2 += 1;
                                        } else {
                                            state.records.insert(game.id.clone(), (0, 0, 1));
                                        }
                                    } else {
                                        if let Some(record) = state.records.get_mut(&game.id) {
                                            if winner == our.node {
                                                record.0 += 1;
                                            } else {
                                                record.1 += 1;
                                            }
                                        } else {
                                            if winner == our.node {
                                                state.records.insert(game.id.clone(), (1, 0, 0));
                                            } else {
                                                state.records.insert(game.id.clone(), (0, 1, 0));
                                            }
                                        }
                                    }
                                }

                                send_ws_update(our.node.clone(), game.clone());
                                save_chess_state(state.clone());

                                send_response(
                                    &Response {
                                        inherit: false,
                                        ipc: None,
                                        metadata: None,
                                    },
                                    Some(&Payload {
                                        mime: Some("application/octet-stream".to_string()),
                                        bytes: "success".as_bytes().to_vec(),
                                    }),
                                );
                                continue;
                            } else {
                                send_response(
                                    &Response {
                                        inherit: false,
                                        ipc: None,
                                        metadata: None,
                                    },
                                    Some(&Payload {
                                        mime: Some("application/octet-stream".to_string()),
                                        bytes: "invalid move".as_bytes().to_vec(),
                                    }),
                                );
                                continue;
                            }
                        }
                        "end_game" => {
                            // end the game and send WS update, update the standings
                            let Some(game) = state.games.get_mut(&game_id) else {
                                send_response(
                                    &Response {
                                        inherit: false,
                                        ipc: None,
                                        metadata: None,
                                    },
                                    Some(&Payload {
                                        mime: Some("application/octet-stream".to_string()),
                                        bytes: "not found".as_bytes().to_vec(),
                                    }),
                                );
                                continue;
                            };

                            game.ended = true;

                            if let Some(record) = state.records.get_mut(&game.id) {
                                record.0 += 1;
                            } else {
                                state.records.insert(game.id.clone(), (1, 0, 0));
                            }

                            send_ws_update(our.node.clone(), game.clone());
                            save_chess_state(state.clone());

                            send_response(
                                &Response {
                                    inherit: false,
                                    ipc: None,
                                    metadata: None,
                                },
                                Some(&Payload {
                                    mime: Some("application/octet-stream".to_string()),
                                    bytes: "success".as_bytes().to_vec(),
                                }),
                            );
                        }
                        _ => {
                            print_to_terminal(1, "chess: got unexpected action");
                            continue;
                        }
                    }
                } else if source.process.to_string() == "http_server:sys:uqbar" {
                    let path = message_json["path"].as_str().unwrap_or("");
                    let method = message_json["method"].as_str().unwrap_or("");

                    let mut default_headers = HashMap::new();
                    default_headers.insert("Content-Type".to_string(), "text/html".to_string());
                    // Handle incoming http
                    match path {
                        "/" => {
                            send_http_response(
                                200,
                                default_headers.clone(),
                                CHESS_PAGE
                                    .replace("${node}", &our.node)
                                    .replace("${process}", &source.process.to_string())
                                    .replace("${js}", CHESS_JS)
                                    .replace("${css}", CHESS_CSS)
                                    .to_string()
                                    .as_bytes()
                                    .to_vec(),
                            );
                        }
                        "/games" => {
                            match method {
                                "GET" => {
                                    send_http_response(
                                        200,
                                        {
                                            let mut headers = default_headers.clone();
                                            headers.insert(
                                                "Content-Type".to_string(),
                                                "application/json".to_string(),
                                            );
                                            headers
                                        },
                                        {
                                            let mut json_games: HashMap<String, serde_json::Value> =
                                                HashMap::new();
                                            for (id, game) in &state.games {
                                                json_games.insert(id.to_string(), json_game(&game));
                                            }
                                            json!(json_games).to_string().as_bytes().to_vec()
                                        },
                                    );
                                }
                                "POST" => {
                                    // create a new game
                                    if let Some(payload) = get_payload() {
                                        if let Ok(payload_json) =
                                            serde_json::from_slice::<serde_json::Value>(
                                                &payload.bytes,
                                            )
                                        {
                                            let game_id = String::from(
                                                payload_json["id"].as_str().unwrap_or(""),
                                            );
                                            if game_id == "" {
                                                send_http_response(
                                                    400,
                                                    default_headers.clone(),
                                                    "Bad Request".to_string().as_bytes().to_vec(),
                                                );
                                                continue;
                                            }

                                            if let Some(game) = state.games.get(&game_id) {
                                                if !game.ended {
                                                    send_http_response(
                                                        409,
                                                        default_headers.clone(),
                                                        "Conflict".to_string().as_bytes().to_vec(),
                                                    );
                                                    continue;
                                                }
                                            }

                                            let white = payload_json["white"]
                                                .as_str()
                                                .unwrap_or(our.node.as_str())
                                                .to_string();
                                            let black = payload_json["black"]
                                                .as_str()
                                                .unwrap_or(game_id.as_str())
                                                .to_string();

                                            let response = send_and_await_response(
                                                &Address {
                                                    node: game_id.clone(),
                                                    process: ProcessId::from_str("chess:chess:uqbar")
                                                        .unwrap(),
                                                },
                                                &Request {
                                                    inherit: false,
                                                    expects_response: Some(30), // TODO check this!
                                                    ipc: Some(
                                                        serde_json::json!({
                                                            "action": "new_game",
                                                            "white": white.clone(),
                                                            "black": black.clone(),
                                                        })
                                                        .to_string(),
                                                    ),
                                                    metadata: None,
                                                },
                                                None,
                                            );

                                            match response {
                                                Ok(_reponse) => {
                                                    if !response_success() {
                                                        send_http_response(
                                                            503,
                                                            default_headers.clone(),
                                                            "Service Unavailable"
                                                                .to_string()
                                                                .as_bytes()
                                                                .to_vec(),
                                                        );
                                                        continue;
                                                    }
                                                    // create a new game
                                                    let game = Game {
                                                        id: game_id.clone(),
                                                        turns: 0,
                                                        board: Board::start_pos(),
                                                        white: white.clone(),
                                                        black: black.clone(),
                                                        ended: false,
                                                    };
                                                    state
                                                        .games
                                                        .insert(game_id.clone(), game.clone());

                                                    save_chess_state(state.clone());

                                                    send_http_response(
                                                        200,
                                                        {
                                                            let mut headers =
                                                                default_headers.clone();
                                                            headers.insert(
                                                                "Content-Type".to_string(),
                                                                "application/json".to_string(),
                                                            );
                                                            headers
                                                        },
                                                        json_game(&game)
                                                            .to_string()
                                                            .as_bytes()
                                                            .to_vec(),
                                                    );
                                                }
                                                Err(_) => {
                                                    send_http_response(
                                                        503,
                                                        default_headers.clone(),
                                                        "Service Unavailable"
                                                            .to_string()
                                                            .as_bytes()
                                                            .to_vec(),
                                                    );
                                                }
                                            }
                                            continue;
                                        }
                                    }

                                    send_http_response(
                                        400,
                                        default_headers.clone(),
                                        "Bad Request".to_string().as_bytes().to_vec(),
                                    );
                                }
                                "PUT" => {
                                    // make a move
                                    if let Some(payload) = get_payload() {
                                        print_to_terminal(
                                            1,
                                            format!(
                                                "payload: {}",
                                                String::from_utf8(payload.bytes.clone())
                                                    .unwrap_or("".to_string())
                                            )
                                            .as_str(),
                                        );
                                        if let Ok(payload_json) =
                                            serde_json::from_slice::<serde_json::Value>(
                                                &payload.bytes,
                                            )
                                        {
                                            let game_id = String::from(
                                                payload_json["id"].as_str().unwrap_or(""),
                                            );

                                            if game_id == "" {
                                                send_http_response(
                                                    400,
                                                    default_headers.clone(),
                                                    "No game ID".to_string().as_bytes().to_vec(),
                                                );
                                                continue;
                                            }

                                            if let Some(game) = state.games.get_mut(&game_id) {
                                                if game.turns % 2 == 0 && game.white != our.node {
                                                    send_http_response(
                                                        403,
                                                        default_headers.clone(),
                                                        "Forbidden".to_string().as_bytes().to_vec(),
                                                    );
                                                    continue;
                                                } else if game.turns % 2 == 1
                                                    && game.black != our.node
                                                {
                                                    send_http_response(
                                                        403,
                                                        default_headers.clone(),
                                                        "Forbidden".to_string().as_bytes().to_vec(),
                                                    );
                                                    continue;
                                                } else if game.ended {
                                                    send_http_response(
                                                        409,
                                                        default_headers.clone(),
                                                        "Conflict".to_string().as_bytes().to_vec(),
                                                    );
                                                    continue;
                                                }

                                                let move_str =
                                                    payload_json["move"].as_str().unwrap_or("");
                                                let valid_move =
                                                    game.board.apply_uci_move(move_str);
                                                if valid_move {
                                                    // send the move to the other player
                                                    // check if the game is over
                                                    // if so, update the records
                                                    let response = send_and_await_response(
                                                        &Address {
                                                            node: game_id.clone(),
                                                            process: ProcessId::from_str(
                                                                "chess:chess:uqbar",
                                                            )
                                                            .unwrap(),
                                                        },
                                                        &Request {
                                                            inherit: false,
                                                            expects_response: Some(30), // TODO check this!
                                                            ipc: Some(
                                                                serde_json::json!({
                                                                    "action": "make_move",
                                                                    "move": move_str,
                                                                })
                                                                .to_string(),
                                                            ),
                                                            metadata: None,
                                                        },
                                                        None,
                                                    );

                                                    match response {
                                                        Ok(_reponse) => {
                                                            if !response_success() {
                                                                send_http_response(
                                                                    503,
                                                                    default_headers.clone(),
                                                                    "Service Unavailable"
                                                                        .to_string()
                                                                        .as_bytes()
                                                                        .to_vec(),
                                                                );
                                                                continue;
                                                            }
                                                            // update the game
                                                            game.turns += 1;
                                                            let checkmate = game.board.checkmate();
                                                            let draw = game.board.stalemate();

                                                            if checkmate || draw {
                                                                game.ended = true;
                                                                let winner = if checkmate {
                                                                    if game.turns % 2 == 1 {
                                                                        game.white.clone()
                                                                    } else {
                                                                        game.black.clone()
                                                                    }
                                                                } else {
                                                                    "".to_string()
                                                                };

                                                                // update the records
                                                                if draw {
                                                                    if let Some(record) = state
                                                                        .records
                                                                        .get_mut(&game.id)
                                                                    {
                                                                        record.2 += 1;
                                                                    } else {
                                                                        state.records.insert(
                                                                            game.id.clone(),
                                                                            (0, 0, 1),
                                                                        );
                                                                    }
                                                                } else {
                                                                    if let Some(record) = state
                                                                        .records
                                                                        .get_mut(&game.id)
                                                                    {
                                                                        if winner == our.node {
                                                                            record.0 += 1;
                                                                        } else {
                                                                            record.1 += 1;
                                                                        }
                                                                    } else {
                                                                        if winner == our.node {
                                                                            state.records.insert(
                                                                                game.id.clone(),
                                                                                (1, 0, 0),
                                                                            );
                                                                        } else {
                                                                            state.records.insert(
                                                                                game.id.clone(),
                                                                                (0, 1, 0),
                                                                            );
                                                                        }
                                                                    }
                                                                }
                                                            }

                                                            let game = game.clone();
                                                            save_chess_state(state.clone());
                                                            // return the game
                                                            send_http_response(
                                                                200,
                                                                {
                                                                    let mut headers =
                                                                        default_headers.clone();
                                                                    headers.insert(
                                                                        "Content-Type".to_string(),
                                                                        "application/json"
                                                                            .to_string(),
                                                                    );
                                                                    headers
                                                                },
                                                                json_game(&game)
                                                                    .to_string()
                                                                    .as_bytes()
                                                                    .to_vec(),
                                                            );
                                                        }
                                                        Err(_) => {
                                                            send_http_response(
                                                                503,
                                                                default_headers.clone(),
                                                                "Service Unavailable"
                                                                    .to_string()
                                                                    .as_bytes()
                                                                    .to_vec(),
                                                            );
                                                        }
                                                    }

                                                    continue;
                                                }
                                            }
                                        }
                                    }

                                    print_to_terminal(0, "never got a response");
                                    send_http_response(
                                        400,
                                        default_headers.clone(),
                                        "Bad Request".to_string().as_bytes().to_vec(),
                                    );
                                }
                                "DELETE" => {
                                    let game_id = message_json["query_params"]["id"]
                                        .as_str()
                                        .unwrap_or("")
                                        .to_string();
                                    if game_id == "" {
                                        send_http_response(
                                            400,
                                            default_headers.clone(),
                                            "Bad Request".to_string().as_bytes().to_vec(),
                                        );
                                        continue;
                                    } else {
                                        if let Some(game) = state.games.get_mut(&game_id) {
                                            let response = send_and_await_response(
                                                &Address {
                                                    node: game_id.clone(),
                                                    process: ProcessId::from_str("chess:chess:uqbar")
                                                        .unwrap(),
                                                },
                                                &Request {
                                                    inherit: false,
                                                    expects_response: Some(30), // TODO check this!
                                                    ipc: Some(
                                                        serde_json::json!({
                                                            "action": "end_game",
                                                        })
                                                        .to_string(),
                                                    ),
                                                    metadata: None,
                                                },
                                                None,
                                            );

                                            match response {
                                                Ok(_response) => {
                                                    if !response_success() {
                                                        send_http_response(
                                                            503,
                                                            default_headers.clone(),
                                                            "Service Unavailable"
                                                                .to_string()
                                                                .as_bytes()
                                                                .to_vec(),
                                                        );
                                                        continue;
                                                    }

                                                    game.ended = true;

                                                    if let Some(record) =
                                                        state.records.get_mut(&game.id)
                                                    {
                                                        record.1 += 1;
                                                    } else {
                                                        state
                                                            .records
                                                            .insert(game.id.clone(), (0, 1, 0));
                                                    }

                                                    let game = game.clone();
                                                    save_chess_state(state.clone());

                                                    // return the game
                                                    send_http_response(
                                                        200,
                                                        {
                                                            let mut headers =
                                                                default_headers.clone();
                                                            headers.insert(
                                                                "Content-Type".to_string(),
                                                                "application/json".to_string(),
                                                            );
                                                            headers
                                                        },
                                                        json_game(&game)
                                                            .to_string()
                                                            .as_bytes()
                                                            .to_vec(),
                                                    );
                                                }
                                                Err(_) => {
                                                    send_http_response(
                                                        503,
                                                        default_headers.clone(),
                                                        "Service Unavailable"
                                                            .to_string()
                                                            .as_bytes()
                                                            .to_vec(),
                                                    );
                                                }
                                            }

                                            continue;
                                        }
                                    }
                                    // end a game
                                }
                                _ => {
                                    send_http_response(
                                        404,
                                        default_headers.clone(),
                                        "Not Found".to_string().as_bytes().to_vec(),
                                    );
                                    continue;
                                }
                            }
                        }
                        _ => {
                            send_http_response(
                                404,
                                default_headers.clone(),
                                "Not Found".to_string().as_bytes().to_vec(),
                            );
                            continue;
                        }
                    }
                }
            }
        }
    }
}
