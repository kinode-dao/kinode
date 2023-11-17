use serde::{Deserialize, Serialize};
use serde_json::json;
use std::collections::HashMap;
extern crate base64;
extern crate pleco;
use pleco::Board;
use uqbar_process_lib::uqbar::process::standard as wit;
use uqbar_process_lib::{
    get_payload, get_typed_state, grant_messaging, println, receive, set_state, Address, Message,
    Payload, ProcessId, Request, Response,
};

wit_bindgen::generate!({
    path: "../../wit",
    world: "process",
    exports: {
        world: Component,
    },
});

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

fn send_http_response(
    status: u16,
    headers: HashMap<String, String>,
    payload_bytes: Vec<u8>,
) -> anyhow::Result<()> {
    Response::new()
        .ipc_bytes(
            serde_json::json!({
                "status": status,
                "headers": headers,
            })
            .to_string()
            .as_bytes()
            .to_vec(),
        )
        .payload(Payload {
            mime: Some("application/octet-stream".to_string()),
            bytes: payload_bytes,
        })
        .send()
}

fn send_ws_update(our: Address, game: Game) -> anyhow::Result<()> {
    Request::new()
        .target(Address::new(&our.node, "encryptor:sys:uqbar").unwrap())?
        .ipc_bytes(
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
                "data": json_game(&game),
            })
            .to_string()
            .as_bytes()
            .to_vec(),
        })
        .send()
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
    set_state(&bincode::serialize(&stored_state).unwrap());
}

const CHESS_PAGE: &str = include_str!("../pkg/chess.html");
const CHESS_JS: &str = include_str!("../pkg/index.js");
const CHESS_CSS: &str = include_str!("../pkg/index.css");

impl Guest for Component {
    fn init(our: String) {
        let our = Address::from_str(&our).unwrap();
        println!("chess: start");

        grant_messaging(
            &our,
            &Vec::from([ProcessId::from_str("http_server:sys:uqbar").unwrap()]),
        );

        for path in ["/", "/games"] {
            Request::new()
                .target(Address::new(&our.node, "http_server:sys:uqbar").unwrap())
                .unwrap()
                .ipc_bytes(
                    serde_json::json!({
                        "BindPath": {
                            "path": path,
                            "authenticated": true,
                            "local_only": false
                        }
                    })
                    .to_string()
                    .as_bytes()
                    .to_vec(),
                )
                .send();
        }

        let mut state: ChessState =
            match get_typed_state(|bytes| Ok(bincode::deserialize::<StoredChessState>(bytes)?)) {
                Some(state) => {
                    let mut games = HashMap::new();
                    for (id, game) in state.games {
                        if let Ok(board) = Board::from_fen(&game.board) {
                            games.insert(
                                id,
                                Game {
                                    id: game.id.clone(),
                                    turns: game.turns,
                                    board,
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
                println!("chess: got network error");
                continue;
            };
            let Message::Request(request) = message else {
                println!("chess: got unexpected Response");
                continue;
            };

            match handle_request(&our, &source, &request, &mut state) {
                Ok(_) => {}
                Err(e) => {
                    println!("chess: error handling request: {:?}", e);
                }
            }
        }
    }
}

fn handle_request(
    our: &Address,
    source: &Address,
    request: &wit::Request,
    state: &mut ChessState,
) -> anyhow::Result<()> {
    let message_json: serde_json::Value = match serde_json::from_slice(&request.ipc) {
        Ok(v) => v,
        Err(_) => return Err(anyhow::anyhow!("chess: failed to parse ipc JSON, skipping")),
    };

    // print_to_terminal(1, &format!("chess: parsed ipc JSON: {:?}", message_json));

    if source.process == "chess:chess:uqbar" {
        let action = message_json["action"].as_str().unwrap_or("");
        let game_id = source.node.clone();

        match action {
            "new_game" => {
                // make a new game with source.node if the current game has ended
                if let Some(game) = state.games.get(&game_id) {
                    if !game.ended {
                        return Response::new()
                            .ipc_bytes(vec![])
                            .payload(Payload {
                                mime: Some("application/octet-stream".to_string()),
                                bytes: "conflict".as_bytes().to_vec(),
                            })
                            .send();
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

                send_ws_update(our.clone(), game.clone());

                save_chess_state(state.clone());

                Response::new()
                    .ipc_bytes(vec![])
                    .payload(Payload {
                        mime: Some("application/octet-stream".to_string()),
                        bytes: "success".as_bytes().to_vec(),
                    })
                    .send()
            }
            "make_move" => {
                // check the move and then update if correct and send WS update
                let Some(game) = state.games.get_mut(&game_id) else {
                    return Response::new()
                        .ipc_bytes(vec![])
                        .payload(Payload {
                            mime: Some("application/octet-stream".to_string()),
                            bytes: "not found".as_bytes().to_vec(),
                        })
                        .send();
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

                    send_ws_update(our.clone(), game.clone());
                    save_chess_state(state.clone());

                    Response::new()
                        .ipc_bytes(vec![])
                        .payload(Payload {
                            mime: Some("application/octet-stream".to_string()),
                            bytes: "success".as_bytes().to_vec(),
                        })
                        .send()
                } else {
                    Response::new()
                        .ipc_bytes(vec![])
                        .payload(Payload {
                            mime: Some("application/octet-stream".to_string()),
                            bytes: "invalid move".as_bytes().to_vec(),
                        })
                        .send()
                }
            }
            "end_game" => {
                // end the game and send WS update, update the standings
                let Some(game) = state.games.get_mut(&game_id) else {
                    return Response::new()
                        .ipc_bytes(vec![])
                        .payload(Payload {
                            mime: Some("application/octet-stream".to_string()),
                            bytes: "not found".as_bytes().to_vec(),
                        })
                        .send();
                };

                game.ended = true;

                if let Some(record) = state.records.get_mut(&game.id) {
                    record.0 += 1;
                } else {
                    state.records.insert(game.id.clone(), (1, 0, 0));
                }

                send_ws_update(our.clone(), game.clone());
                save_chess_state(state.clone());

                Response::new()
                    .ipc_bytes(vec![])
                    .payload(Payload {
                        mime: Some("application/octet-stream".to_string()),
                        bytes: "success".as_bytes().to_vec(),
                    })
                    .send()
            }
            _ => return Err(anyhow::anyhow!("chess: got unexpected action")),
        }
    } else if source.process.to_string() == "http_server:sys:uqbar" {
        let path = message_json["path"].as_str().unwrap_or("");
        let method = message_json["method"].as_str().unwrap_or("");

        let mut default_headers = HashMap::new();
        default_headers.insert("Content-Type".to_string(), "text/html".to_string());
        // Handle incoming http
        match path {
            "/" => {
                return send_http_response(
                    200,
                    default_headers.clone(),
                    CHESS_PAGE
                        .replace("${node}", &our.node)
                        .replace("${process}", &our.process.to_string())
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
                        return send_http_response(
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
                                serde_json::from_slice::<serde_json::Value>(&payload.bytes)
                            {
                                let game_id =
                                    String::from(payload_json["id"].as_str().unwrap_or(""));
                                if game_id == "" {
                                    return send_http_response(
                                        400,
                                        default_headers.clone(),
                                        "Bad Request".to_string().as_bytes().to_vec(),
                                    );
                                }

                                if let Some(game) = state.games.get(&game_id) {
                                    if !game.ended {
                                        return send_http_response(
                                            409,
                                            default_headers.clone(),
                                            "Conflict".to_string().as_bytes().to_vec(),
                                        );
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

                                let response = Request::new()
                                    .target(Address::new(&game_id, "chess:chess:uqbar")?)?
                                    .ipc_bytes(
                                        serde_json::json!({
                                            "action": "new_game",
                                            "white": white.clone(),
                                            "black": black.clone(),
                                        })
                                        .to_string()
                                        .as_bytes()
                                        .to_vec(),
                                    )
                                    .send_and_await_response(30)?;

                                match response {
                                    Ok(_response) => {
                                        if !response_success() {
                                            return send_http_response(
                                                503,
                                                default_headers.clone(),
                                                "Service Unavailable"
                                                    .to_string()
                                                    .as_bytes()
                                                    .to_vec(),
                                            );
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
                                        state.games.insert(game_id.clone(), game.clone());

                                        save_chess_state(state.clone());

                                        return send_http_response(
                                            200,
                                            {
                                                let mut headers = default_headers.clone();
                                                headers.insert(
                                                    "Content-Type".to_string(),
                                                    "application/json".to_string(),
                                                );
                                                headers
                                            },
                                            json_game(&game).to_string().as_bytes().to_vec(),
                                        );
                                    }
                                    Err(_) => {
                                        return send_http_response(
                                            503,
                                            default_headers.clone(),
                                            "Service Unavailable".to_string().as_bytes().to_vec(),
                                        )
                                    }
                                }
                            }
                        }
                        return send_http_response(
                            400,
                            default_headers.clone(),
                            "Bad Request".to_string().as_bytes().to_vec(),
                        );
                    }
                    "PUT" => {
                        // make a move
                        if let Some(payload) = get_payload() {
                            if let Ok(payload_json) =
                                serde_json::from_slice::<serde_json::Value>(&payload.bytes)
                            {
                                let game_id =
                                    String::from(payload_json["id"].as_str().unwrap_or(""));

                                if game_id == "" {
                                    return send_http_response(
                                        400,
                                        default_headers.clone(),
                                        "No game ID".to_string().as_bytes().to_vec(),
                                    );
                                }

                                if let Some(game) = state.games.get_mut(&game_id) {
                                    if (game.turns % 2 == 0 && game.white != our.node)
                                        || (game.turns % 2 == 1 && game.black != our.node)
                                    {
                                        return send_http_response(
                                            403,
                                            default_headers.clone(),
                                            "Forbidden".to_string().as_bytes().to_vec(),
                                        );
                                    } else if game.ended {
                                        return send_http_response(
                                            409,
                                            default_headers.clone(),
                                            "Conflict".to_string().as_bytes().to_vec(),
                                        );
                                    }

                                    let move_str = payload_json["move"].as_str().unwrap_or("");
                                    let valid_move = game.board.apply_uci_move(move_str);
                                    if valid_move {
                                        // send the move to the other player
                                        // check if the game is over
                                        // if so, update the records
                                        let response = Request::new()
                                            .target(Address::new(&game_id, "chess:chess:uqbar")?)?
                                            .ipc_bytes(
                                                serde_json::json!({
                                                    "action": "make_move",
                                                    "move": move_str,
                                                })
                                                .to_string()
                                                .as_bytes()
                                                .to_vec(),
                                            )
                                            .send_and_await_response(30)?;

                                        match response {
                                            Ok(_response) => {
                                                if !response_success() {
                                                    return send_http_response(
                                                        503,
                                                        default_headers.clone(),
                                                        "Service Unavailable"
                                                            .to_string()
                                                            .as_bytes()
                                                            .to_vec(),
                                                    );
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
                                                        if let Some(record) =
                                                            state.records.get_mut(&game.id)
                                                        {
                                                            record.2 += 1;
                                                        } else {
                                                            state
                                                                .records
                                                                .insert(game.id.clone(), (0, 0, 1));
                                                        }
                                                    } else {
                                                        if let Some(record) =
                                                            state.records.get_mut(&game.id)
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
                                                return send_http_response(
                                                    200,
                                                    {
                                                        let mut headers = default_headers.clone();
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
                                                return send_http_response(
                                                    503,
                                                    default_headers.clone(),
                                                    "Service Unavailable"
                                                        .to_string()
                                                        .as_bytes()
                                                        .to_vec(),
                                                )
                                            }
                                        }
                                    }
                                }
                            }
                        }

                        println!("chess: never got a response");
                        return send_http_response(
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
                            return send_http_response(
                                400,
                                default_headers.clone(),
                                "Bad Request".to_string().as_bytes().to_vec(),
                            );
                        } else {
                            let Some(game) = state.games.get_mut(&game_id) else {
                                return send_http_response(
                                    400,
                                    default_headers.clone(),
                                    "Bad Request".to_string().as_bytes().to_vec(),
                                );
                            };
                            let response = Request::new()
                                .target(Address::new(&game_id, "chess:chess:uqbar")?)?
                                .ipc_bytes(
                                    serde_json::json!({
                                        "action": "end_game",
                                    })
                                    .to_string()
                                    .as_bytes()
                                    .to_vec(),
                                )
                                .send_and_await_response(30)?;

                            match response {
                                Ok(_response) => {
                                    if !response_success() {
                                        return send_http_response(
                                            503,
                                            default_headers.clone(),
                                            "Service Unavailable".to_string().as_bytes().to_vec(),
                                        );
                                    }

                                    game.ended = true;

                                    if let Some(record) = state.records.get_mut(&game.id) {
                                        record.1 += 1;
                                    } else {
                                        state.records.insert(game.id.clone(), (0, 1, 0));
                                    }

                                    let game = game.clone();
                                    save_chess_state(state.clone());

                                    // return the game
                                    return send_http_response(
                                        200,
                                        {
                                            let mut headers = default_headers.clone();
                                            headers.insert(
                                                "Content-Type".to_string(),
                                                "application/json".to_string(),
                                            );
                                            headers
                                        },
                                        json_game(&game).to_string().as_bytes().to_vec(),
                                    );
                                }
                                Err(_) => {
                                    return send_http_response(
                                        503,
                                        default_headers.clone(),
                                        "Service Unavailable".to_string().as_bytes().to_vec(),
                                    );
                                }
                            }
                        }
                    }
                    _ => {
                        return send_http_response(
                            404,
                            default_headers.clone(),
                            "Not Found".to_string().as_bytes().to_vec(),
                        )
                    }
                }
            }
            _ => {
                return send_http_response(
                    404,
                    default_headers.clone(),
                    "Not Found".to_string().as_bytes().to_vec(),
                )
            }
        }
    } else {
        return Err(anyhow::anyhow!("chess: got request from unexpected source"));
    }
}
