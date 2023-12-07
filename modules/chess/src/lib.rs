#![feature(let_chains)]
use serde::{Deserialize, Serialize};
use std::collections::{HashMap, HashSet};
extern crate base64;
extern crate pleco;
use pleco::Board;
use uqbar_process_lib::uqbar::process::standard as wit;
use uqbar_process_lib::{
    get_payload, get_typed_state, http, println, receive, set_state, Address, Message, Payload,
    Request, Response,
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

#[derive(Debug, Serialize, Deserialize)]
enum ChessRequest {
    NewGame { white: String, black: String },
    Move(String), // can only have one game with a given node at a time
    Resign,
}

#[derive(Debug, Eq, PartialEq, Serialize, Deserialize)]
enum ChessResponse {
    NewGameAccepted,
    NewGameRejected,
    MoveAccepted,
    MoveRejected,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
struct Game {
    pub id: String, // the node with whom we are playing
    pub turns: u64,
    pub board: String,
    pub white: String,
    pub black: String,
    pub ended: bool,
}

#[derive(Debug, Serialize, Deserialize)]
struct ChessState {
    pub games: HashMap<String, Game>, // game is by opposing player id
    pub clients: HashSet<u32>,        // doesn't get persisted
}

#[derive(Debug, Serialize, Deserialize)]
struct StoredChessState {
    pub games: HashMap<String, Game>, // game is by opposing player id
}

const CHESS_HTML: &str = include_str!("../pkg/chess.html");
const CHESS_JS: &str = include_str!("../pkg/index.js");
const CHESS_CSS: &str = include_str!("../pkg/index.css");

impl Guest for Component {
    fn init(our: String) {
        let our = Address::from_str(&our).unwrap();
        println!(
            "{} by {}: start",
            our.process.process_name, our.process.publisher_node
        );

        // serve static page at /index.html, /index.js, /index.css
        // dynamically handle requests to /games
        http::bind_http_static_path(
            "/",
            true,  // only serve for ourselves
            false, // can access remotely
            Some("text/html".to_string()),
            CHESS_HTML
                .replace("${node}", &our.node)
                .replace("${process}", &our.process.to_string())
                .as_bytes()
                .to_vec(),
        )
        .unwrap();
        http::bind_http_static_path(
            "/index.js",
            true,
            false,
            Some("text/javascript".to_string()),
            CHESS_JS.as_bytes().to_vec(),
        )
        .unwrap();
        http::bind_http_static_path(
            "/index.css",
            true,
            false,
            Some("text/css".to_string()),
            CHESS_CSS.as_bytes().to_vec(),
        )
        .unwrap();
        http::bind_http_path("/games", true, false).unwrap();

        // serve same content SECURELY at a subdomain:

        let _res = Request::new()
            .target(("our", "http_server", "sys", "uqbar"))
            .ipc(
                serde_json::to_vec(&http::HttpServerAction::SecureBind {
                    path: "/secure/index.html".into(),
                    cache: true,
                })
                .unwrap(),
            )
            .payload(Payload {
                mime: Some("text/html".to_string()),
                bytes: CHESS_HTML
                    .replace("${node}", &our.node)
                    .replace("${process}", &our.process.to_string())
                    .as_bytes()
                    .to_vec(),
            })
            .send_and_await_response(5)
            .unwrap();
        let _res = Request::new()
            .target(("our", "http_server", "sys", "uqbar"))
            .ipc(
                serde_json::to_vec(&http::HttpServerAction::SecureBind {
                    path: "/secure/index.css".into(),
                    cache: true,
                })
                .unwrap(),
            )
            .payload(Payload {
                mime: Some("text/css".to_string()),
                bytes: CHESS_CSS.as_bytes().to_vec(),
            })
            .send_and_await_response(5)
            .unwrap();
        let _res = Request::new()
            .target(("our", "http_server", "sys", "uqbar"))
            .ipc(
                serde_json::to_vec(&http::HttpServerAction::SecureBind {
                    path: "/secure/index.js".into(),
                    cache: true,
                })
                .unwrap(),
            )
            .payload(Payload {
                mime: Some("text/javascript".to_string()),
                bytes: CHESS_JS.as_bytes().to_vec(),
            })
            .send_and_await_response(5)
            .unwrap();
        let _res = Request::new()
            .target(("our", "http_server", "sys", "uqbar"))
            .ipc(
                serde_json::to_vec(&http::HttpServerAction::SecureBind {
                    path: "/games".into(),
                    cache: false,
                })
                .unwrap(),
            )
            .send_and_await_response(5)
            .unwrap();

        let mut state: ChessState = utils::load_chess_state();
        main_loop(&our, &mut state);
    }
}

fn main_loop(our: &Address, state: &mut ChessState) {
    loop {
        let Ok((source, message)) = receive() else {
            println!("{our}: got network error");
            continue;
        };
        // we don't expect any responses *here*, because for every
        // chess protocol request, we await its response right then and
        // there. this is appropriate for direct node<>node comms, less
        // appropriate for other circumstances...
        let Message::Request(request) = message else {
            // println!("{our}: got unexpected Response from {source}: {message:?}");
            continue;
        };
        match handle_request(&our, &source, &request, state) {
            Ok(()) => continue,
            Err(e) => println!("{our}: error handling request: {:?}", e),
        }
    }
}

/// handle all incoming requests
fn handle_request(
    our: &Address,
    source: &Address,
    request: &wit::Request,
    state: &mut ChessState,
) -> anyhow::Result<()> {
    println!("{}: handling request from {}", our.process.process_name, source);
    if source.process == our.process && source.node != our.node {
        // receive chess protocol messages from other nodes
        let chess_request = serde_json::from_slice::<ChessRequest>(&request.ipc)?;
        handle_chess_request(our, source, state, chess_request)
    } else if source.process == "http_server:sys:uqbar" && source.node == our.node {
        // receive HTTP requests and websocket connection messages from our server
        match serde_json::from_slice::<http::HttpServerRequest>(&request.ipc)? {
            http::HttpServerRequest::Http(incoming) => {
                match handle_http_request(our, state, incoming) {
                    Ok(()) => Ok(()),
                    Err(e) => {
                        println!("chess: error handling http request: {:?}", e);
                        http::send_response(
                            http::StatusCode::SERVICE_UNAVAILABLE,
                            None,
                            "Service Unavailable".to_string().as_bytes().to_vec(),
                        )
                    }
                }
            }
            http::HttpServerRequest::WebSocketOpen(channel_id) => {
                // client frontend opened a websocket
                state.clients.insert(channel_id);
                Ok(())
            }
            http::HttpServerRequest::WebSocketClose(channel_id) => {
                // client frontend closed a websocket
                state.clients.remove(&channel_id);
                Ok(())
            }
            http::HttpServerRequest::WebSocketPush { message_type, .. } => {
                // client frontend sent a websocket message
                // we don't expect this! we only use websockets to push updates
                // Err(anyhow::anyhow!("got unexpected websocket message!"))
                Ok(())
            }
        }
    } else {
        Err(anyhow::anyhow!("got unexpected request from {source}"))
    }
}

/// handle chess protocol messages from other nodes
fn handle_chess_request(
    our: &Address,
    source: &Address,
    state: &mut ChessState,
    action: ChessRequest,
) -> anyhow::Result<()> {
    let game_id = &source.node;
    match action {
        ChessRequest::NewGame { white, black } => {
            // make a new game with source.node
            // this will replace any existing game with source.node!
            if state.games.contains_key(game_id) {
                println!("chess: resetting game with {game_id} on their request!");
            }
            let game = Game {
                id: game_id.to_string(),
                turns: 0,
                board: Board::start_pos().fen(),
                white,
                black,
                ended: false,
            };
            utils::send_ws_update(&our, &game, &state.clients)?;
            state.games.insert(game_id.to_string(), game);
            utils::save_chess_state(&state);
            // tell them we've accepted the game
            // at the moment, we do not reject any new game requests!
            Response::new()
                .ipc(serde_json::to_vec(&ChessResponse::NewGameAccepted)?)
                .send()
        }
        ChessRequest::Move(ref move_str) => {
            // check the move and then update if correct and send WS update
            let Some(game) = state.games.get_mut(game_id) else {
                // if we don't have a game with them, reject the move
                return Response::new()
                    .ipc(serde_json::to_vec(&ChessResponse::MoveRejected)?)
                    .send()
            };
            let mut board = Board::from_fen(&game.board).unwrap();
            if !board.apply_uci_move(move_str) {
                // reject invalid moves!
                return Response::new()
                    .ipc(serde_json::to_vec(&ChessResponse::MoveRejected)?)
                    .send();
            }
            game.turns += 1;
            if board.checkmate() || board.stalemate() {
                game.ended = true;
            }
            game.board = board.fen();
            utils::send_ws_update(&our, &game, &state.clients)?;
            utils::save_chess_state(&state);
            Response::new()
                .ipc(serde_json::to_vec(&ChessResponse::MoveAccepted)?)
                .send()
        }
        ChessRequest::Resign => {
            let Some(game) = state.games.get_mut(game_id) else {
                return Response::new()
                    .ipc(serde_json::to_vec(&ChessResponse::MoveRejected)?)
                    .send()
            };
            game.ended = true;
            utils::send_ws_update(&our, &game, &state.clients)?;
            utils::save_chess_state(&state);
            // we don't respond to these
            Ok(())
        }
    }
}

/// handle HTTP requests from our own frontend
fn handle_http_request(
    our: &Address,
    state: &mut ChessState,
    http_request: http::IncomingHttpRequest,
) -> anyhow::Result<()> {
    if http_request.path()? != "games" {
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
            serde_json::to_vec(&state.games)?,
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
                .target((game_id, our.process.clone()))
                .ipc(serde_json::to_vec(&ChessRequest::NewGame {
                    white: player_white.clone(),
                    black: player_black.clone(),
                })?)
                .send_and_await_response(5)?;
            // if they accept, create a new game
            // otherwise, should surface error to FE...
            let Ok((_source, Message::Response((resp, _context)))) = response else {
                return Err(anyhow::anyhow!("other player did not respond properly to new game request"));
            };
            let resp = serde_json::from_slice::<ChessResponse>(&resp.ipc)?;
            if resp != ChessResponse::NewGameAccepted {
                return Err(anyhow::anyhow!("other player rejected new game request"));
            }
            // create a new game
            let game = Game {
                id: game_id.to_string(),
                turns: 0,
                board: Board::start_pos().fen(),
                white: player_white,
                black: player_black,
                ended: false,
            };
            let body = serde_json::to_vec(&game)?;
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
            let Some(move_str) = payload_json["move"].as_str() else {
                return http::send_response(http::StatusCode::BAD_REQUEST, None, vec![]);
            };
            let mut board = Board::from_fen(&game.board).unwrap();
            if !board.apply_uci_move(move_str) {
                // TODO surface illegal move to player or something here
                return http::send_response(http::StatusCode::BAD_REQUEST, None, vec![]);
            }
            // send the move to the other player
            // check if the game is over
            // if so, update the records
            let response = Request::new()
                .target((game_id, our.process.clone()))
                .ipc(serde_json::to_vec(&ChessRequest::Move(
                    move_str.to_string(),
                ))?)
                .send_and_await_response(5)?;
            let Ok((_source, Message::Response((resp, _context)))) = response else {
                return Err(anyhow::anyhow!("other player did not respond properly to new game request"));
            };
            let resp = serde_json::from_slice::<ChessResponse>(&resp.ipc)?;
            if resp != ChessResponse::MoveAccepted {
                return Err(anyhow::anyhow!("other player rejected new game request"));
            }
            // update the game
            game.turns += 1;
            if board.checkmate() || board.stalemate() {
                game.ended = true;
            }
            game.board = board.fen();
            // update state and return to FE
            let body = serde_json::to_vec(&game)?;
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
            // end the game
            let Some(game_id) = http_request.query_params.get("id") else {
                return http::send_response(http::StatusCode::BAD_REQUEST, None, vec![]);
            };
            let Some(game) = state.games.get_mut(game_id) else {
                return http::send_response(http::StatusCode::BAD_REQUEST, None, vec![]);
            };
            // send the other player an end game request
            Request::new()
                .target((game_id.as_str(), our.process.clone()))
                .ipc(serde_json::to_vec(&ChessRequest::Resign)?)
                .send()?;
            game.ended = true;
            let body = serde_json::to_vec(&game)?;
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
        _ => http::send_response(http::StatusCode::METHOD_NOT_ALLOWED, None, vec![]),
    }
}
