#![feature(let_chains)]
use crate::kinode::process::chess::{
    MoveRequest, NewGameRequest, Request as ChessRequest, Response as ChessResponse,
};
use kinode_process_lib::{
    await_message, call_init, get_blob, get_typed_state, http, http::server, println, set_state,
    Address, LazyLoadBlob, Message, NodeId, Request, Response,
};
use pleco::Board;
use serde::{Deserialize, Serialize};
use std::collections::{HashMap, HashSet};

const ICON: &str = include_str!("icon");

///
/// Our serializable state format.
///
#[derive(Clone, Debug, Serialize, Deserialize)]
struct Game {
    /// the node with whom we are playing
    pub id: String,
    pub turns: u64,
    pub board: String,
    pub white: String,
    pub black: String,
    pub ended: bool,
}

#[derive(Debug, Serialize, Deserialize)]
struct ChessState {
    pub our: Address,
    pub games: HashMap<String, Game>, // game is by opposing player id
    pub clients: HashSet<u32>,        // doesn't get persisted
}

#[derive(Debug, Serialize, Deserialize)]
struct StoredChessState {
    pub games: HashMap<String, Game>, // game is by opposing player id
}

fn save_chess_state(state: &ChessState) {
    set_state(&bincode::serialize(&state.games).unwrap());
}

fn load_chess_state(our: Address) -> ChessState {
    match get_typed_state(|bytes| bincode::deserialize::<HashMap<String, Game>>(bytes)) {
        Some(games) => ChessState {
            our,
            games,
            clients: HashSet::new(),
        },
        None => {
            let state = ChessState {
                our,
                games: HashMap::new(),
                clients: HashSet::new(),
            };
            save_chess_state(&state);
            state
        }
    }
}

fn send_ws_update(http_server: &mut server::HttpServer, game: &Game) {
    http_server.ws_push_all_channels(
        "/",
        server::WsMessageType::Binary,
        LazyLoadBlob {
            mime: Some("application/json".to_string()),
            bytes: serde_json::json!({
                "kind": "game_update",
                "data": game,
            })
            .to_string()
            .into_bytes(),
        },
    )
}

// Boilerplate: generate the wasm bindings for a process
wit_bindgen::generate!({
    path: "target/wit",
    world: "chess-sys-v0",
    generate_unused_types: true,
    additional_derives: [PartialEq, serde::Deserialize, serde::Serialize],
});

// After generating bindings, use this macro to define the Component struct
// and its init() function, which the kernel will look for on startup.
call_init!(initialize);
fn initialize(our: Address) {
    // A little printout to show in terminal that the process has started.
    println!("started");

    // add ourselves to the homepage
    kinode_process_lib::homepage::add_to_homepage("Chess", Some(ICON), Some("/"), None);

    // create an HTTP server struct with which to manipulate `http-server:distro:sys`
    let mut http_server = server::HttpServer::new(5);
    let http_config = server::HttpBindingConfig::default();

    // Serve the index.html and other UI files found in pkg/ui at the root path.
    // authenticated=true, local_only=false
    http_server
        .serve_ui("ui", vec!["/"], http_config.clone())
        .expect("failed to serve ui");

    // Allow HTTP requests to be made to /games; they will be handled dynamically.
    http_server
        .bind_http_path("/games", http_config.clone())
        .expect("failed to bind /games");

    // Allow websockets to be opened at / (our process ID will be prepended).
    http_server
        .bind_ws_path("/", server::WsBindingConfig::default())
        .expect("failed to bind ws");

    // Grab our state, then enter the main event loop.
    let mut state: ChessState = load_chess_state(our);
    main_loop(&mut state, &mut http_server);
}

fn main_loop(state: &mut ChessState, http_server: &mut server::HttpServer) {
    loop {
        // Call await_message() to wait for any incoming messages.
        // If we get a network error, make a print and throw it away.
        // In a high-quality consumer-grade app, we'd want to explicitly handle
        // this and surface it to the user.
        match await_message() {
            Err(send_error) => {
                println!("got network error: {send_error:?}");
                continue;
            }
            Ok(message) => match handle_request(&message, state, http_server) {
                Ok(()) => continue,
                Err(e) => println!("error handling request: {e}"),
            },
        }
    }
}

/// Handle chess protocol messages from ourself *or* other nodes.
fn handle_request(
    message: &Message,
    state: &mut ChessState,
    http_server: &mut server::HttpServer,
) -> anyhow::Result<()> {
    // Throw away responses. We never expect any responses *here*, because for every
    // chess protocol request, we *await* its response in-place. This is appropriate
    // for direct node-to-node comms, less appropriate for other circumstances...
    if !message.is_request() {
        return Ok(());
    }

    // If the request is from another node, handle it as an incoming request.
    // Note that we can enforce the ProcessId as well, but it shouldn't be a trusted
    // piece of information, since another node can easily spoof any ProcessId on a request.
    // It can still be useful simply as a protocol-level switch to handle different kinds of
    // requests from the same node, with the knowledge that the remote node can finagle with
    // which ProcessId a given message can be from. It's their code, after all.
    if message.source().node != state.our.node {
        // Deserialize the request IPC to our format, and throw it away if it
        // doesn't fit.
        let Ok(chess_request) = serde_json::from_slice::<ChessRequest>(message.body()) else {
            return Err(anyhow::anyhow!("invalid chess request"));
        };
        handle_chess_request(&message.source().node, state, http_server, &chess_request)
    }
    // ...and if the request is from ourselves, handle it as our own!
    // Note that since this is a local request, we *can* trust the ProcessId.
    else {
        // Here, we accept messages *from any local process that can message this one*.
        // Since the manifest specifies that this process is *public*, any local process
        // can "play chess" for us.
        //
        // If you wanted to restrict this privilege, you could check for a specific process,
        // package, and/or publisher here, *or* change the manifest to only grant messaging
        // capabilities to specific processes.

        // if the message is from the HTTP server runtime module, we should handle it
        // as an HTTP request and not a chess request
        if message.source().process == "http-server:distro:sys" {
            return handle_http_request(state, http_server, message);
        }

        let Ok(chess_request) = serde_json::from_slice::<ChessRequest>(message.body()) else {
            return Err(anyhow::anyhow!("invalid chess request"));
        };
        let _game = handle_local_request(state, &chess_request)?;
        Ok(())
    }
}

/// Handle chess protocol messages from other nodes.
fn handle_chess_request(
    source_node: &NodeId,
    state: &mut ChessState,
    http_server: &mut server::HttpServer,
    action: &ChessRequest,
) -> anyhow::Result<()> {
    println!("handling action from {source_node}: {action:?}");

    // For simplicity's sake, we'll just use the node we're playing with as the game id.
    // This limits us to one active game per partner.
    let game_id = source_node;

    match action {
        ChessRequest::NewGame(NewGameRequest { white, black }) => {
            // Make a new game with source.node
            // This will replace any existing game with source.node!
            if state.games.contains_key(game_id) {
                println!("chess: resetting game with {game_id} on their request!");
            }
            let game = Game {
                id: game_id.to_string(),
                turns: 0,
                board: Board::start_pos().fen(),
                white: white.to_string(),
                black: black.to_string(),
                ended: false,
            };
            // Use our helper function to persist state after every action.
            // The simplest and most trivial way to keep state. You'll want to
            // use a database or something in a real app, and consider performance
            // when doing intensive data-based operations.
            send_ws_update(http_server, &game);
            state.games.insert(game_id.to_string(), game);
            save_chess_state(&state);
            // Send a response to tell them we've accepted the game.
            // Remember, the other player is waiting for this.
            Response::new()
                .body(serde_json::to_vec(&ChessResponse::NewGameAccepted)?)
                .send()?;
            Ok(())
        }
        ChessRequest::Move(MoveRequest { ref move_str, .. }) => {
            // Get the associated game, and respond with an error if
            // we don't have it in our state.
            let Some(game) = state.games.get_mut(game_id) else {
                // If we don't have a game with them, reject the move.
                Response::new()
                    .body(serde_json::to_vec(&ChessResponse::MoveRejected)?)
                    .send()?;
                return Ok(());
            };
            // Convert the saved board to one we can manipulate.
            let mut board = Board::from_fen(&game.board).unwrap();
            if !board.apply_uci_move(move_str) {
                // Reject invalid moves!
                Response::new()
                    .body(serde_json::to_vec(&ChessResponse::MoveRejected)?)
                    .send()?;
                return Ok(());
            }
            game.turns += 1;
            if board.checkmate() || board.stalemate() {
                game.ended = true;
            }
            // Persist state.
            game.board = board.fen();
            send_ws_update(http_server, &game);
            save_chess_state(&state);
            // Send a response to tell them we've accepted the move.
            Response::new()
                .body(serde_json::to_vec(&ChessResponse::MoveAccepted)?)
                .send()?;
            Ok(())
        }
        ChessRequest::Resign(_) => {
            // They've resigned. The sender isn't waiting for a response to this,
            // so we don't need to send one.
            match state.games.get_mut(game_id) {
                Some(game) => {
                    game.ended = true;
                    send_ws_update(http_server, &game);
                    save_chess_state(&state);
                }
                None => {}
            }
            Ok(())
        }
    }
}

/// Handle actions we are performing. Here's where we'll send_and_await various requests.
fn handle_local_request(state: &mut ChessState, action: &ChessRequest) -> anyhow::Result<Game> {
    match action {
        ChessRequest::NewGame(NewGameRequest { white, black }) => {
            // Create a new game. We'll enforce that one of the two players is us.
            if white != &state.our.node && black != &state.our.node {
                return Err(anyhow::anyhow!("cannot start a game without us!"));
            }
            let game_id = if white == &state.our.node {
                black
            } else {
                white
            };
            // If we already have a game with this player, throw an error.
            if let Some(game) = state.games.get(game_id)
                && !game.ended
            {
                return Err(anyhow::anyhow!("already have a game with {game_id}"));
            };
            // Send the other player a NewGame request
            // The request is exactly the same as what we got from terminal.
            // We'll give their node 30 seconds to respond...
            let Ok(Message::Response { ref body, .. }) = Request::new()
                .target((game_id, state.our.process.clone()))
                .body(serde_json::to_vec(&action)?)
                .send_and_await_response(30)?
            else {
                return Err(anyhow::anyhow!(
                    "other player did not respond properly to new game request"
                ));
            };
            // If they accept, create a new game -- otherwise, error out.
            if serde_json::from_slice::<ChessResponse>(body)? != ChessResponse::NewGameAccepted {
                return Err(anyhow::anyhow!("other player rejected new game request!"));
            }
            // New game with default board.
            let game = Game {
                id: game_id.to_string(),
                turns: 0,
                board: Board::start_pos().fen(),
                white: white.to_string(),
                black: black.to_string(),
                ended: false,
            };
            state.games.insert(game_id.to_string(), game.clone());
            save_chess_state(&state);
            Ok(game)
        }
        ChessRequest::Move(MoveRequest { game_id, move_str }) => {
            // Make a move. We'll enforce that it's our turn. The game_id is the
            // person we're playing with.
            let Some(game) = state.games.get_mut(game_id) else {
                return Err(anyhow::anyhow!("no game with {game_id}"));
            };
            if (game.turns % 2 == 0 && game.white != state.our.node)
                || (game.turns % 2 == 1 && game.black != state.our.node)
            {
                return Err(anyhow::anyhow!("not our turn!"));
            } else if game.ended {
                return Err(anyhow::anyhow!("that game is over!"));
            }
            let mut board = Board::from_fen(&game.board).unwrap();
            if !board.apply_uci_move(move_str) {
                return Err(anyhow::anyhow!("illegal move!"));
            }
            // Send the move to the other player, then check if the game is over.
            // The request is exactly the same as what we got from terminal.
            // We'll give their node 30 seconds to respond...
            let Ok(Message::Response { ref body, .. }) = Request::new()
                .target((game_id, state.our.process.clone()))
                .body(serde_json::to_vec(&action)?)
                .send_and_await_response(30)?
            else {
                return Err(anyhow::anyhow!(
                    "other player did not respond properly to our move"
                ));
            };
            if serde_json::from_slice::<ChessResponse>(body)? != ChessResponse::MoveAccepted {
                return Err(anyhow::anyhow!("other player rejected our move"));
            }
            game.turns += 1;
            if board.checkmate() || board.stalemate() {
                game.ended = true;
            }
            game.board = board.fen();
            let game = game.clone();
            save_chess_state(&state);
            Ok(game)
        }
        ChessRequest::Resign(ref with_who) => {
            // Resign from a game with a given player.
            let Some(game) = state.games.get_mut(with_who) else {
                return Err(anyhow::anyhow!("no game with {with_who}"));
            };
            // send the other player an end game request -- no response expected
            Request::new()
                .target((with_who, state.our.process.clone()))
                .body(serde_json::to_vec(&action)?)
                .send()?;
            game.ended = true;
            let game = game.clone();
            save_chess_state(&state);
            Ok(game)
        }
    }
}

/// Handle HTTP requests from our own frontend.
fn handle_http_request(
    state: &mut ChessState,
    http_server: &mut server::HttpServer,
    message: &Message,
) -> anyhow::Result<()> {
    let request = http_server.parse_request(message.body())?;

    // the HTTP server helper struct allows us to pass functions that
    // handle the various types of requests we get from the frontend
    http_server.handle_request(
        request,
        |incoming| {
            // client frontend sent an HTTP request, process it and
            // return an HTTP response
            // these functions can reuse the logic from handle_local_request
            // after converting the request into the appropriate format!
            match incoming.method().unwrap_or_default() {
                http::Method::GET => handle_get(state),
                http::Method::POST => handle_post(state),
                http::Method::PUT => handle_put(state),
                http::Method::DELETE => handle_delete(state, &incoming),
                _ => (
                    server::HttpResponse::new(http::StatusCode::METHOD_NOT_ALLOWED),
                    None,
                ),
            }
        },
        |_channel_id, _message_type, _message| {
            // client frontend sent a websocket message
            // we don't expect this! we only use websockets to push updates
        },
    );

    Ok(())
}

/// On GET: return all active games
fn handle_get(state: &mut ChessState) -> (server::HttpResponse, Option<LazyLoadBlob>) {
    (
        server::HttpResponse::new(http::StatusCode::OK),
        Some(LazyLoadBlob {
            mime: Some("application/json".to_string()),
            bytes: serde_json::to_vec(&state.games).expect("failed to serialize games!"),
        }),
    )
}

/// On POST: create a new game
fn handle_post(state: &mut ChessState) -> (server::HttpResponse, Option<LazyLoadBlob>) {
    let Some(blob) = get_blob() else {
        return (
            server::HttpResponse::new(http::StatusCode::BAD_REQUEST),
            None,
        );
    };
    let Ok(blob_json) = serde_json::from_slice::<serde_json::Value>(&blob.bytes) else {
        return (
            server::HttpResponse::new(http::StatusCode::BAD_REQUEST),
            None,
        );
    };
    let Some(game_id) = blob_json["id"].as_str() else {
        return (
            server::HttpResponse::new(http::StatusCode::BAD_REQUEST),
            None,
        );
    };

    let player_white = blob_json["white"]
        .as_str()
        .unwrap_or(state.our.node.as_str())
        .to_string();
    let player_black = blob_json["black"].as_str().unwrap_or(game_id).to_string();

    match handle_local_request(
        state,
        &ChessRequest::NewGame(NewGameRequest {
            white: player_white,
            black: player_black,
        }),
    ) {
        Ok(game) => (
            server::HttpResponse::new(http::StatusCode::OK)
                .header("Content-Type", "application/json"),
            Some(LazyLoadBlob {
                mime: Some("application/json".to_string()),
                bytes: serde_json::to_vec(&game).expect("failed to serialize game!"),
            }),
        ),
        Err(e) => (
            server::HttpResponse::new(http::StatusCode::BAD_REQUEST),
            Some(LazyLoadBlob {
                mime: Some("application/text".to_string()),
                bytes: e.to_string().into_bytes(),
            }),
        ),
    }
}

/// On PUT: make a move
fn handle_put(state: &mut ChessState) -> (server::HttpResponse, Option<LazyLoadBlob>) {
    let Some(blob) = get_blob() else {
        return (
            server::HttpResponse::new(http::StatusCode::BAD_REQUEST),
            None,
        );
    };
    let Ok(blob_json) = serde_json::from_slice::<serde_json::Value>(&blob.bytes) else {
        return (
            server::HttpResponse::new(http::StatusCode::BAD_REQUEST),
            None,
        );
    };

    let Some(game_id) = blob_json["id"].as_str() else {
        return (
            server::HttpResponse::new(http::StatusCode::BAD_REQUEST),
            None,
        );
    };
    let Some(move_str) = blob_json["move"].as_str() else {
        return (
            server::HttpResponse::new(http::StatusCode::BAD_REQUEST),
            None,
        );
    };

    match handle_local_request(
        state,
        &ChessRequest::Move(MoveRequest {
            game_id: game_id.to_string(),
            move_str: move_str.to_string(),
        }),
    ) {
        Ok(game) => (
            server::HttpResponse::new(http::StatusCode::OK)
                .header("Content-Type", "application/json"),
            Some(LazyLoadBlob {
                mime: Some("application/json".to_string()),
                bytes: serde_json::to_vec(&game).expect("failed to serialize game!"),
            }),
        ),
        Err(e) => (
            server::HttpResponse::new(http::StatusCode::BAD_REQUEST),
            Some(LazyLoadBlob {
                mime: Some("application/text".to_string()),
                bytes: e.to_string().into_bytes(),
            }),
        ),
    }
}

/// On DELETE: end the game
fn handle_delete(
    state: &mut ChessState,
    request: &server::IncomingHttpRequest,
) -> (server::HttpResponse, Option<LazyLoadBlob>) {
    let Some(game_id) = request.query_params().get("id") else {
        return (
            server::HttpResponse::new(http::StatusCode::BAD_REQUEST),
            None,
        );
    };
    match handle_local_request(state, &ChessRequest::Resign(game_id.to_string())) {
        Ok(game) => (
            server::HttpResponse::new(http::StatusCode::OK)
                .header("Content-Type", "application/json"),
            Some(LazyLoadBlob {
                mime: Some("application/json".to_string()),
                bytes: serde_json::to_vec(&game).expect("failed to serialize game!"),
            }),
        ),
        Err(e) => (
            server::HttpResponse::new(http::StatusCode::BAD_REQUEST),
            Some(LazyLoadBlob {
                mime: Some("application/text".to_string()),
                bytes: e.to_string().into_bytes(),
            }),
        ),
    }
}
