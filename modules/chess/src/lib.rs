#![feature(let_chains)]
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
extern crate base64;
extern crate pleco;
use pleco::Board;
use uqbar_process_lib::uqbar::process::standard as wit;
use uqbar_process_lib::{
    get_payload, get_typed_state, grant_messaging, http, println, receive, set_state, Address,
    Message, Payload, ProcessId, Request, Response,
};

mod utils;

wit_bindgen::generate!({
    path: "../../wit",
    world: "process",
    exports: {
        world: Component,
    },
});

struct Component;

#[derive(Clone, Debug)]
pub struct Game {
    pub id: String, // the node with whom we are playing
    pub turns: u64,
    pub board: Board,
    pub white: String,
    pub black: String,
    pub ended: bool,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct StoredGame {
    pub id: String, // the node with whom we are playing
    pub turns: u64,
    pub board: String,
    pub white: String,
    pub black: String,
    pub ended: bool,
}

#[derive(Clone, Debug)]
pub struct ChessState {
    pub games: HashMap<String, Game>, // game is by opposing player id
    pub records: HashMap<String, (u64, u64, u64)>, // wins, losses, draws
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct StoredChessState {
    pub games: HashMap<String, StoredGame>, // game is by opposing player id
    pub records: HashMap<String, (u64, u64, u64)>, // wins, losses, draws
}

const CHESS_PAGE: &str = include_str!("../pkg/chess.html");
const CHESS_JS: &str = include_str!("../pkg/index.js");
const CHESS_CSS: &str = include_str!("../pkg/index.css");

impl Guest for Component {
    fn init(our: String) {
        let our = Address::from_str(&our).unwrap();
        println!("{our}: start");

        grant_messaging(
            &our,
            vec![ProcessId::new(Some("http_server"), "sys", "uqbar")],
        );

        // serve static page at /
        // dynamically handle requests to /games
        http::bind_http_static_path(
            "/",
            true,
            false,
            Some("text/html".to_string()),
            CHESS_PAGE
                .replace("${node}", &our.node)
                .replace("${process}", &our.process.to_string())
                // TODO serve these independently on paths..
                // also build utils for just serving a vfs dir
                .replace("${js}", CHESS_JS)
                .replace("${css}", CHESS_CSS)
                .as_bytes()
                .to_vec(),
        )
        .unwrap();
        http::bind_http_path("/games", true, false).unwrap();

        let mut state: ChessState = match get_typed_state(|bytes| {
            Ok(bincode::deserialize::<StoredChessState>(bytes)?)
        }) {
            Some(mut state) => ChessState {
                games: state
                    .games
                    .iter_mut()
                    .map(|(id, game)| {
                        (
                            id.clone(),
                            Game {
                                id: id.to_owned(),
                                turns: game.turns,
                                board: Board::from_fen(&game.board).unwrap_or(Board::start_pos()),
                                white: game.white.to_owned(),
                                black: game.black.to_owned(),
                                ended: game.ended,
                            },
                        )
                    })
                    .collect(),
                records: state.records,
            },
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
                Ok(()) => continue,
                Err(e) => println!("chess: error handling request: {:?}", e),
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
    if source.process == "chess:chess:uqbar" {
        let message_json = serde_json::from_slice::<serde_json::Value>(&request.ipc)?;
        handle_chess_request(our, source, message_json, state)
    } else if source.process.to_string() == "http_server:sys:uqbar" {
        let http_request = serde_json::from_slice::<http::IncomingHttpRequest>(&request.ipc)?;
        handle_http_request(our, http_request, state)
    } else {
        return Err(anyhow::anyhow!("chess: got request from unexpected source"));
    }
}

fn handle_chess_request(
    our: &Address,
    source: &Address,
    message_json: serde_json::Value,
    state: &mut ChessState,
) -> anyhow::Result<()> {
    let action = message_json["action"].as_str().unwrap_or("");
    let game_id = &source.node;
    match action {
        "new_game" => {
            // make a new game with source.node if the current game has ended
            if let Some(game) = state.games.get(game_id) {
                if !game.ended {
                    return Response::new()
                        .ipc(vec![])
                        .payload(Payload {
                            mime: Some("application/octet-stream".to_string()),
                            bytes: "conflict".as_bytes().to_vec(),
                        })
                        .send();
                }
            }
            let game = Game {
                id: game_id.to_string(),
                turns: 0,
                board: Board::start_pos(),
                white: message_json["white"]
                    .as_str()
                    .unwrap_or(game_id)
                    .to_string(),
                black: message_json["black"]
                    .as_str()
                    .unwrap_or(&our.node)
                    .to_string(),
                ended: false,
            };
            state.games.insert(game_id.to_string(), game.clone());

            utils::send_ws_update(&our, &game)?;
            utils::save_chess_state(&state);

            Response::new()
                .ipc(vec![])
                .payload(Payload {
                    mime: Some("application/octet-stream".to_string()),
                    bytes: "success".as_bytes().to_vec(),
                })
                .send()
        }
        "make_move" => {
            // check the move and then update if correct and send WS update
            let Some(game) = state.games.get_mut(game_id) else {
                return Response::new()
                    .ipc(vec![])
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

                utils::send_ws_update(&our, &game)?;
                utils::save_chess_state(&state);

                Response::new()
                    .ipc(vec![])
                    .payload(Payload {
                        mime: Some("application/octet-stream".to_string()),
                        bytes: "success".as_bytes().to_vec(),
                    })
                    .send()
            } else {
                Response::new()
                    .ipc(vec![])
                    .payload(Payload {
                        mime: Some("application/octet-stream".to_string()),
                        bytes: "invalid move".as_bytes().to_vec(),
                    })
                    .send()
            }
        }
        "end_game" => {
            // end the game and send WS update, update the standings
            let Some(game) = state.games.get_mut(game_id) else {
                return Response::new()
                    .ipc(vec![])
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

            utils::send_ws_update(&our, &game)?;
            utils::save_chess_state(&state);

            Response::new()
                .ipc(vec![])
                .payload(Payload {
                    mime: Some("application/octet-stream".to_string()),
                    bytes: "success".as_bytes().to_vec(),
                })
                .send()
        }
        _ => return Err(anyhow::anyhow!("chess: got unexpected action")),
    }
}

fn handle_http_request(
    our: &Address,
    http_request: http::IncomingHttpRequest,
    state: &mut ChessState,
) -> anyhow::Result<()> {
    if http_request.path()? != "/games" {
        return http::send_response(
            http::StatusCode::NOT_FOUND,
            None,
            "Not Found".to_string().as_bytes().to_vec(),
        );
    }
    match http_request.method.as_str() {
        "GET" => http::send_response(
            http::StatusCode::OK,
            Some(HashMap::from([(
                String::from("Content-Type"),
                String::from("application/json"),
            )])),
            serde_json::to_vec(&serde_json::json!(state
                .games
                .iter()
                .map(|(id, game)| (id.to_string(), utils::json_game(game)))
                .collect::<HashMap<String, serde_json::Value>>()))?,
        ),
        "POST" => {
            // create a new game
            let Some(payload) = get_payload() else {
                return http::send_response(http::StatusCode::BAD_REQUEST, None, vec![]);
            };
            let payload_json = serde_json::from_slice::<serde_json::Value>(&payload.bytes)?;
            let Some(game_id) = payload_json["id"].as_str() else {
                return http::send_response(http::StatusCode::BAD_REQUEST, None, vec![]);
            };
            if let Some(game) = state.games.get(game_id)
                && !game.ended
            {
                return http::send_response(http::StatusCode::CONFLICT, None, vec![]);
            };

            let player_white = payload_json["white"]
                .as_str()
                .unwrap_or(our.node.as_str())
                .to_string();
            let player_black = payload_json["black"]
                .as_str()
                .unwrap_or(game_id)
                .to_string();

            // send the other player a new game request
            let response = Request::new()
                .target((game_id, "chess", "chess", "uqbar"))
                .ipc(serde_json::to_vec(&serde_json::json!({
                    "action": "new_game",
                    "white": player_white.clone(),
                    "black": player_black.clone(),
                }))?)
                .send_and_await_response(30)?;
            // if they accept, create a new game
            // otherwise, should surface error to FE...
            let Ok((_source, Message::Response((resp, _context)))) = response else {
                return http::send_response(
                    http::StatusCode::SERVICE_UNAVAILABLE,
                    None,
                    "Service Unavailable".to_string().as_bytes().to_vec(),
                );
            };
            if resp.ipc != "success".as_bytes() {
                return http::send_response(http::StatusCode::SERVICE_UNAVAILABLE, None, vec![]);
            }
            // create a new game
            let game = Game {
                id: game_id.to_string(),
                turns: 0,
                board: Board::start_pos(),
                white: player_white,
                black: player_black,
                ended: false,
            };
            let body = serde_json::to_vec(&utils::json_game(&game))?;
            state.games.insert(game_id.to_string(), game);
            utils::save_chess_state(&state);
            http::send_response(
                http::StatusCode::OK,
                Some(HashMap::from([(
                    String::from("Content-Type"),
                    String::from("application/json"),
                )])),
                body,
            )
        }
        "PUT" => {
            // make a move
            let Some(payload) = get_payload() else {
                return http::send_response(http::StatusCode::BAD_REQUEST, None, vec![]);
            };
            let payload_json = serde_json::from_slice::<serde_json::Value>(&payload.bytes)?;
            let Some(game_id) = payload_json["id"].as_str() else {
                return http::send_response(http::StatusCode::BAD_REQUEST, None, vec![]);
            };
            let Some(game) = state.games.get_mut(game_id) else {
                return http::send_response(http::StatusCode::NOT_FOUND, None, vec![]);
            };
            if (game.turns % 2 == 0 && game.white != our.node)
                || (game.turns % 2 == 1 && game.black != our.node)
            {
                return http::send_response(http::StatusCode::FORBIDDEN, None, vec![]);
            } else if game.ended {
                return http::send_response(http::StatusCode::CONFLICT, None, vec![]);
            }
            let move_str = payload_json["move"].as_str().unwrap_or("");
            if !game.board.apply_uci_move(move_str) {
                // TODO surface illegal move to player or something here
                return http::send_response(http::StatusCode::BAD_REQUEST, None, vec![]);
            }
            // send the move to the other player
            // check if the game is over
            // if so, update the records
            let response = Request::new()
                .target((game_id, "chess", "chess", "uqbar"))
                .ipc(serde_json::to_vec(&serde_json::json!({
                    "action": "make_move",
                    "move": move_str,
                }))?)
                .send_and_await_response(30)?;
            let Ok((_source, Message::Response((resp, _context)))) = response else {
                // TODO surface error to player, let them know other player is
                // offline or whatever they respond here was invalid
                return http::send_response(http::StatusCode::BAD_REQUEST, None, vec![]);
            };
            if resp.ipc != "success".as_bytes() {
                return http::send_response(http::StatusCode::SERVICE_UNAVAILABLE, None, vec![]);
            }
            // update the game
            game.turns += 1;
            let checkmate = game.board.checkmate();
            let draw = game.board.stalemate();

            if checkmate || draw {
                game.ended = true;
                let winner = if checkmate {
                    if game.turns % 2 == 1 {
                        &game.white
                    } else {
                        &game.black
                    }
                } else {
                    ""
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
            // game is not over, update state and return to FE
            let body = serde_json::to_vec(&utils::json_game(&game))?;
            utils::save_chess_state(&state);
            // return the game
            http::send_response(
                http::StatusCode::OK,
                Some(HashMap::from([(
                    String::from("Content-Type"),
                    String::from("application/json"),
                )])),
                body,
            )
        }
        "DELETE" => {
            // "end the game"?
            let query_params = http_request.query_params()?;
            let Some(game_id) = query_params.get("id") else {
                return http::send_response(http::StatusCode::BAD_REQUEST, None, vec![]);
            };
            let Some(game) = state.games.get_mut(game_id) else {
                return http::send_response(http::StatusCode::BAD_REQUEST, None, vec![]);
            };
            // send the other player an end game request
            let response = Request::new()
                .target((game_id, "chess", "chess", "uqbar"))
                .ipc(serde_json::to_vec(&serde_json::json!({
                    "action": "end_game",
                }))?)
                .send_and_await_response(30)?;
            let Ok((_source, Message::Response((resp, _context)))) = response else {
                // TODO surface error to player, let them know other player is
                // offline or whatever they respond here was invalid
                return http::send_response(http::StatusCode::SERVICE_UNAVAILABLE, None, vec![]);
            };
            if resp.ipc != "success".as_bytes() {
                return http::send_response(http::StatusCode::SERVICE_UNAVAILABLE, None, vec![]);
            }

            game.ended = true;
            if let Some(record) = state.records.get_mut(&game.id) {
                record.1 += 1;
            } else {
                state.records.insert(game.id.clone(), (0, 1, 0));
            }
            // return the game
            let body = serde_json::to_vec(&utils::json_game(&game))?;
            utils::save_chess_state(&state);

            http::send_response(
                http::StatusCode::OK,
                Some(HashMap::from([(
                    String::from("Content-Type"),
                    String::from("application/json"),
                )])),
                body,
            )
        }
        _ => Response::new()
            .ipc(serde_json::to_vec(&http::HttpResponse {
                status: 405,
                headers: HashMap::new(),
            })?)
            .send(),
    }
}
