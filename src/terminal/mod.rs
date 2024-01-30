use crate::types::*;
use anyhow::Result;
use chrono::{Datelike, Local, Timelike};
use crossterm::{
    cursor,
    event::{
        DisableBracketedPaste, EnableBracketedPaste, Event, EventStream, KeyCode, KeyEvent,
        KeyModifiers,
    },
    execute,
    style::Print,
    terminal::{self, disable_raw_mode, enable_raw_mode, ClearType},
};
use futures::{future::FutureExt, StreamExt};
use std::fs::{read_to_string, OpenOptions};
use std::io::{stdout, BufWriter, Write};
use tokio::signal::unix::{signal, SignalKind};

mod utils;

struct RawMode;
impl RawMode {
    fn new() -> anyhow::Result<Self> {
        enable_raw_mode()?;
        Ok(RawMode)
    }
}
impl Drop for RawMode {
    fn drop(&mut self) {
        match disable_raw_mode() {
            Ok(_) => {
                // let is_enabled = crossterm::terminal::is_raw_mode_enabled();
                // println!("terminal: disabled raw mode successfully: {is_enabled:?}\r");
            }
            Err(e) => {
                println!("terminal: failed to disable raw mode: {e:?}\r");
            }
        }
    }
}

/*
 *  terminal driver
 */
pub async fn terminal(
    our: Identity,
    version: &str,
    home_directory_path: String,
    event_loop: MessageSender,
    debug_event_loop: DebugSender,
    print_tx: PrintSender,
    mut print_rx: PrintReceiver,
    is_detached: bool,
) -> Result<()> {
    let mut stdout = stdout();
    execute!(
        stdout,
        EnableBracketedPaste,
        terminal::SetTitle(format!("{}", our.name))
    )?;

    let (mut win_cols, mut win_rows) = terminal::size().unwrap();
    // print initial splash screen, large if there's room, small otherwise
    if win_cols >= 90 {
        println!(
            "\x1b[38;5;128m{}\x1b[0m",
            format_args!(
                r#"
 ⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⢀⡠⠖⠉
 ⠁⠶⣤⣀⡀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⣀⠔⠋⠀⠀⠀888    d8P  d8b                        888
 ⠀⠀⠈⢛⠿⣷⣦⣤⣀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⣀⡤⠤⣴⡞⠁⠀⠀⠀⠀⠀888   d8P   Y8P                        888
 ⠀⠀⠀⠀⠙⠳⢾⣿⣟⣻⠷⣦⣤⣀⠀⠀⠀⠀⠀⠀⣾⣿⣿⣿⣿⠀⠀⠀⠀⠀⠀⠀888  d8P                               888
 ⠀⠀⠀⠀⠀⠀⠙⠲⣯⣿⣿⣿⣿⠽⢿⣷⣦⣤⣀⠀⢿⣿⣿⣿⣿⠀⠀⠀⠀⠀⠀⠀888d88K     888 88888b.   .d88b.   .d88888  .d88b.
 ⠀⠀⠀⠀⠀⠀⠀⠀⠀⠲⠾⠿⣿⣿⣿⣿⣿⣿⣿⣷⣿⣿⣿⣿⣿⡀⠀⠀⠀⠀⠀⠀8888888b    888 888 "88b d88""88b d88" 888 d8P  Y8b
 ⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠙⠛⣛⣯⣿⣿⣿⣿⣿⢿⣿⣿⣿⣿⡇⠀⠀⠀⠀⠀⠀888  Y88b   888 888  888 888  888 888  888 88888888
 ⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠐⠛⡵⣿⣿⣿⣿⣿⣿⣿⣿⡇⠀⠀⠀⠀⠀⠀888   Y88b  888 888  888 Y88..88P Y88b 888 Y8b.
 ⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠈⣱⣿⣿⣿⣿⣿⣿⡿⠁⠀⠀⠀⠀⠀⠀888    Y88b 888 888  888  "Y88P"   "Y88888  "Y8888
 ⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⢀⣼⣿⣿⣿⣿⣿⡽⠋⠀⠀⠀⠀⠀⠀⠀⠀
 ⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⣰⣿⣿⡿⠿⠛⣫⡽⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀{} ({})
 ⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⣠⣾⣿⣯⣷⡞⠀⠀⠈⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀version {}
 ⠀⠀⠀⠀⠀⠀⠀⠀⠀⢀⣠⡾⣿⡿⣿⣿⣿⠁⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀a general purpose sovereign cloud computer
 ⠀⠀⠀⠀⠀⠀⠀⣠⠴⠛⠉⢰⡿⢱⡿⢹⡟⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀
 ⠀⠀⠀⠀⠀⠀⠉⠀⠀⠀⢠⡿⠁⡿⠁⡿⠃⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀
 ⠀⠀⠀⠀⠀⠀⠀⠀⠀⢠⠟⠀⡸⠁⠀⠁⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀
 ⠀⠀⠀⠀⠀⠀⠀⢀⠔⠁⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀
 networking public key: {}
                "#,
                our.name,
                if our.ws_routing.is_some() {
                    "direct"
                } else {
                    "indirect"
                },
                version,
                our.networking_key,
            )
        );
    } else {
        println!(
            "\x1b[38;5;128m{}\x1b[0m",
            format_args!(
                r#"
 888    d8P  d8b                        888
 888   d8P   Y8P                        888
 888  d8P                               888
 888d88K     888 88888b.   .d88b.   .d88888  .d88b.
 8888888b    888 888 "88b d88""88b d88" 888 d8P  Y8b
 888  Y88b   888 888  888 888  888 888  888 88888888
 888   Y88b  888 888  888 Y88..88P Y88b 888 Y8b.
 888    Y88b 888 888  888  "Y88P"   "Y88888  "Y8888

 {} ({})
 version {}
 a general purpose sovereign cloud computer
 net pubkey: {}
                "#,
                our.name,
                if our.ws_routing.is_some() {
                    "direct"
                } else {
                    "indirect"
                },
                version,
                our.networking_key,
            )
        );
    }

    let _raw_mode = if is_detached {
        None
    } else {
        Some(RawMode::new()?)
    };

    let mut reader = EventStream::new();
    let mut current_line = format!("{} > ", our.name);
    let prompt_len: usize = our.name.len() + 3;
    let mut cursor_col: u16 = prompt_len.try_into().unwrap();
    let mut line_col: usize = cursor_col as usize;
    let mut in_step_through: bool = false;
    let mut verbose_mode: u8 = 0; // least verbose mode
    let mut search_mode: bool = false;
    let mut search_depth: usize = 0;
    let mut logging_mode: bool = false;

    let history_path = std::fs::canonicalize(&home_directory_path)
        .unwrap()
        .join(".terminal_history");
    let history = read_to_string(&history_path).unwrap_or_default();
    let history_handle = OpenOptions::new()
        .append(true)
        .create(true)
        .open(&history_path)
        .unwrap();
    let history_writer = BufWriter::new(history_handle);
    // TODO make adjustable max history length
    let mut command_history = utils::CommandHistory::new(1000, history, history_writer);

    let log_path = std::fs::canonicalize(&home_directory_path)
        .unwrap()
        .join(".terminal_log");
    let log_handle = OpenOptions::new()
        .append(true)
        .create(true)
        .open(&log_path)
        .unwrap();
    let mut log_writer = BufWriter::new(log_handle);

    // use to trigger cleanup if receive signal to kill process
    let mut sigalrm =
        signal(SignalKind::alarm()).expect("terminal: failed to set up SIGALRM handler");
    let mut sighup =
        signal(SignalKind::hangup()).expect("terminal: failed to set up SIGHUP handler");
    let mut sigint =
        signal(SignalKind::interrupt()).expect("terminal: failed to set up SIGINT handler");
    let mut sigpipe =
        signal(SignalKind::pipe()).expect("terminal: failed to set up SIGPIPE handler");
    let mut sigquit =
        signal(SignalKind::quit()).expect("terminal: failed to set up SIGQUIT handler");
    let mut sigterm =
        signal(SignalKind::terminate()).expect("terminal: failed to set up SIGTERM handler");
    let mut sigusr1 =
        signal(SignalKind::user_defined1()).expect("terminal: failed to set up SIGUSR1 handler");
    let mut sigusr2 =
        signal(SignalKind::user_defined2()).expect("terminal: failed to set up SIGUSR2 handler");

    loop {
        let event = reader.next().fuse();

        tokio::select! {
            Some(printout) = print_rx.recv() => {
                let now = Local::now();
                if logging_mode {
                    let _ = writeln!(log_writer, "[{}] {}", now.to_rfc2822(), printout.content);
                }
                if printout.verbosity > verbose_mode {
                    continue;
                }
                let mut stdout = stdout.lock();
                execute!(
                    stdout,
                    cursor::MoveTo(0, win_rows - 1),
                    terminal::Clear(ClearType::CurrentLine),
                    Print(format!("{}{} {}/{} {:02}:{:02} ",
                                   match printout.verbosity {
                                       0 => "",
                                       1 => "1️⃣  ",
                                       2 => "2️⃣  ",
                                       _ => "3️⃣  ",
                                   },
                                   now.weekday(),
                                   now.month(),
                                   now.day(),
                                   now.hour(),
                                   now.minute(),
                                 )),
                )?;
                for line in printout.content.lines() {
                    execute!(
                        stdout,
                        Print(format!("\x1b[38;5;238m{}\x1b[0m\r\n", line)),
                    )?;
                }
                execute!(
                    stdout,
                    cursor::MoveTo(0, win_rows),
                    Print(utils::truncate_in_place(&current_line, prompt_len, win_cols, (line_col, cursor_col))),
                    cursor::MoveTo(cursor_col, win_rows),
                )?;
            }
            Some(Ok(event)) = event => {
                let mut stdout = stdout.lock();
                match event {
                    // resize is super annoying because this event trigger often
                    // comes "too late" to stop terminal from messing with the
                    // already-printed lines. TODO figure out the right way
                    // to compensate for this cross-platform and do this in a
                    // generally stable way.
                    Event::Resize(width, height) => {
                        win_cols = width;
                        win_rows = height;
                    },
                    // handle pasting of text from outside
                    Event::Paste(pasted) => {
                        current_line.insert_str(line_col, &pasted);
                        line_col = current_line.len();
                        cursor_col = std::cmp::min(line_col.try_into().unwrap_or(win_cols), win_cols);
                        execute!(
                            stdout,
                            cursor::MoveTo(0, win_rows),
                            Print(utils::truncate_in_place(&current_line, prompt_len, win_cols, (line_col, cursor_col))),
                            cursor::MoveTo(cursor_col, win_rows),
                        )?;
                    }
                    // CTRL+C, CTRL+D: turn off the node
                    Event::Key(KeyEvent {
                        code: KeyCode::Char('c'),
                        modifiers: KeyModifiers::CONTROL,
                        ..
                    }) |
                    Event::Key(KeyEvent {
                        code: KeyCode::Char('d'),
                        modifiers: KeyModifiers::CONTROL,
                        ..
                    }) => {
                        execute!(stdout, DisableBracketedPaste, terminal::SetTitle(""))?;
                        break;
                    },
                    // CTRL+V: toggle through verbosity modes
                    Event::Key(KeyEvent {
                        code: KeyCode::Char('v'),
                        modifiers: KeyModifiers::CONTROL,
                        ..
                    }) => {
                        // go from low to high, then reset to 0
                        match verbose_mode {
                            0 => verbose_mode = 1,
                            1 => verbose_mode = 2,
                            2 => verbose_mode = 3,
                            _ => verbose_mode = 0,
                        }
                        let _ = print_tx.send(
                            Printout {
                                verbosity: 0,
                                content: match verbose_mode {
                                    0 => "verbose mode: off".into(),
                                    1 => "verbose mode: debug".into(),
                                    2 => "verbose mode: super-debug".into(),
                                    _ => "verbose mode: full event loop".into(),
                                }
                            }
                        ).await;
                    },
                    // CTRL+J: toggle debug mode -- makes system-level event loop step-through
                    // CTRL+S: step through system-level event loop
                    Event::Key(KeyEvent {
                        code: KeyCode::Char('j'),
                        modifiers: KeyModifiers::CONTROL,
                        ..
                    }) => {
                        let _ = print_tx.send(
                            Printout {
                                verbosity: 0,
                                content: match in_step_through {
                                    true => "debug mode off".into(),
                                    false => "debug mode on: use CTRL+S to step through events".into(),
                                }
                            }
                        ).await;
                        let _ = debug_event_loop.send(DebugCommand::Toggle).await;
                        in_step_through = !in_step_through;
                    },
                    Event::Key(KeyEvent {
                        code: KeyCode::Char('s'),
                        modifiers: KeyModifiers::CONTROL,
                        ..
                    }) => {
                        let _ = debug_event_loop.send(DebugCommand::Step).await;
                    },
                    //
                    //  CTRL+L: toggle logging mode
                    //
                    Event::Key(KeyEvent {
                        code: KeyCode::Char('l'),
                        modifiers: KeyModifiers::CONTROL,
                        ..
                    }) => {
                        logging_mode = !logging_mode;
                        let _ = print_tx.send(
                            Printout {
                                verbosity: 0,
                                content: match logging_mode {
                                    true => "logging mode: on".into(),
                                    false => "logging mode: off".into(),
                                }
                            }
                        ).await;
                    },
                    //
                    //  UP / CTRL+P: go up one command in history
                    //  DOWN / CTRL+N: go down one command in history
                    //
                    Event::Key(KeyEvent { code: KeyCode::Up, .. }) |
                    Event::Key(KeyEvent {
                        code: KeyCode::Char('p'),
                        modifiers: KeyModifiers::CONTROL,
                        ..
                    }) => {
                        // go up one command in history
                        match command_history.get_prev(&current_line[prompt_len..]) {
                            Some(line) => {
                                current_line = format!("{} > {}", our.name, line);
                                line_col = current_line.len();
                            },
                            None => {
                                print!("\x07");
                            },
                        }
                        cursor_col = std::cmp::min(current_line.len() as u16, win_cols);
                        execute!(
                            stdout,
                            cursor::MoveTo(0, win_rows),
                            terminal::Clear(ClearType::CurrentLine),
                            Print(utils::truncate_rightward(&current_line, prompt_len, win_cols)),
                        )?;
                    },
                    Event::Key(KeyEvent { code: KeyCode::Down, .. }) |
                    Event::Key(KeyEvent {
                        code: KeyCode::Char('n'),
                        modifiers: KeyModifiers::CONTROL,
                        ..
                    }) => {
                        // go down one command in history
                        match command_history.get_next() {
                            Some(line) => {
                                current_line = format!("{} > {}", our.name, line);
                                line_col = current_line.len();
                            },
                            None => {
                                print!("\x07");
                            },
                        }
                        cursor_col = std::cmp::min(current_line.len() as u16, win_cols);
                        execute!(
                            stdout,
                            cursor::MoveTo(0, win_rows),
                            terminal::Clear(ClearType::CurrentLine),
                            Print(utils::truncate_rightward(&current_line, prompt_len, win_cols)),
                        )?;
                    },
                    //
                    //  CTRL+A: jump to beginning of line
                    //
                    Event::Key(KeyEvent {
                        code: KeyCode::Char('a'),
                        modifiers: KeyModifiers::CONTROL,
                        ..
                    }) => {
                        line_col = prompt_len;
                        cursor_col = prompt_len.try_into().unwrap();
                        execute!(
                            stdout,
                            cursor::MoveTo(0, win_rows),
                            Print(utils::truncate_from_left(&current_line, prompt_len, win_cols, line_col)),
                            cursor::MoveTo(cursor_col, win_rows),
                        )?;
                    },
                    //
                    //  CTRL+E: jump to end of line
                    //
                    Event::Key(KeyEvent {
                        code: KeyCode::Char('e'),
                        modifiers: KeyModifiers::CONTROL,
                        ..
                    }) => {
                        line_col = current_line.len();
                        cursor_col = std::cmp::min(line_col.try_into().unwrap_or(win_cols), win_cols);
                        execute!(
                            stdout,
                            cursor::MoveTo(0, win_rows),
                            Print(utils::truncate_from_right(&current_line, prompt_len, win_cols, line_col)),
                        )?;
                    },
                    //
                    //  CTRL+R: enter search mode
                    //  if already in search mode, increase search depth
                    //
                    Event::Key(KeyEvent {
                        code: KeyCode::Char('r'),
                        modifiers: KeyModifiers::CONTROL,
                        ..
                    }) => {
                        if search_mode {
                            search_depth += 1;
                        }
                        search_mode = true;
                        let search_query = &current_line[prompt_len..];
                        if search_query.is_empty() {
                            continue;
                        }
                        if let Some(result) = command_history.search(search_query, search_depth) {
                            let result_underlined = utils::underline(result, search_query);
                            execute!(
                                stdout,
                                cursor::MoveTo(0, win_rows),
                                terminal::Clear(ClearType::CurrentLine),
                                Print(utils::truncate_in_place(
                                    &format!("{} * {}", our.name, result_underlined),
                                    prompt_len,
                                    win_cols,
                                    (line_col, cursor_col))),
                                cursor::MoveTo(cursor_col, win_rows),
                            )?;
                        } else {
                            execute!(
                                stdout,
                                cursor::MoveTo(0, win_rows),
                                terminal::Clear(ClearType::CurrentLine),
                                Print(utils::truncate_in_place(&current_line, prompt_len, win_cols, (line_col, cursor_col))),
                                cursor::MoveTo(cursor_col, win_rows),
                            )?;
                        }
                    },
                    //
                    //  CTRL+G: exit search mode
                    //
                    Event::Key(KeyEvent {
                        code: KeyCode::Char('g'),
                        modifiers: KeyModifiers::CONTROL,
                        ..
                    }) => {
                        // just show true current line as usual
                        search_mode = false;
                        search_depth = 0;
                        execute!(
                            stdout,
                            cursor::MoveTo(0, win_rows),
                            terminal::Clear(ClearType::CurrentLine),
                            Print(utils::truncate_in_place(&current_line, prompt_len, win_cols, (line_col, cursor_col))),
                            cursor::MoveTo(cursor_col, win_rows),
                        )?;
                    },
                    //
                    //  handle keypress events
                    //
                    Event::Key(k) => {
                        match k.code {
                            KeyCode::Char(c) => {
                                current_line.insert(line_col, c);
                                if cursor_col < win_cols {
                                    cursor_col += 1;
                                }
                                line_col += 1;
                                if search_mode {
                                    let search_query = &current_line[prompt_len..];
                                    if let Some(result) = command_history.search(search_query, search_depth) {
                                        let result_underlined = utils::underline(result, search_query);
                                        execute!(
                                            stdout,
                                            cursor::MoveTo(0, win_rows),
                                            terminal::Clear(ClearType::CurrentLine),
                                            Print(utils::truncate_in_place(
                                                &format!("{} * {}", our.name, result_underlined),
                                                prompt_len,
                                                win_cols,
                                                (line_col, cursor_col))),
                                            cursor::MoveTo(cursor_col, win_rows),
                                        )?;
                                        continue;
                                    }
                                }
                                execute!(
                                    stdout,
                                    cursor::MoveTo(0, win_rows),
                                    terminal::Clear(ClearType::CurrentLine),
                                    Print(utils::truncate_in_place(&current_line, prompt_len, win_cols, (line_col, cursor_col))),
                                    cursor::MoveTo(cursor_col, win_rows),
                                )?;
                            },
                            KeyCode::Backspace => {
                                if line_col == prompt_len {
                                    continue;
                                }
                                if cursor_col as usize == line_col {
                                    cursor_col -= 1;
                                }
                                line_col -= 1;
                                current_line.remove(line_col);
                                if search_mode {
                                    let search_query = &current_line[prompt_len..];
                                    if let Some(result) = command_history.search(search_query, search_depth) {
                                        let result_underlined = utils::underline(result, search_query);
                                        execute!(
                                            stdout,
                                            cursor::MoveTo(0, win_rows),
                                            terminal::Clear(ClearType::CurrentLine),
                                            Print(utils::truncate_in_place(
                                                &format!("{} * {}", our.name, result_underlined),
                                                prompt_len,
                                                win_cols,
                                                (line_col, cursor_col))),
                                            cursor::MoveTo(cursor_col, win_rows),
                                        )?;
                                        continue;
                                    }
                                }
                                execute!(
                                    stdout,
                                    cursor::MoveTo(0, win_rows),
                                    terminal::Clear(ClearType::CurrentLine),
                                    Print(utils::truncate_in_place(&current_line, prompt_len, win_cols, (line_col, cursor_col))),
                                    cursor::MoveTo(cursor_col, win_rows),
                                )?;
                            },
                            KeyCode::Left => {
                                if cursor_col as usize == prompt_len {
                                    if line_col == prompt_len {
                                        // at the very beginning of the current typed line
                                        continue;
                                    } else {
                                        // virtual scroll leftward through line
                                        line_col -= 1;
                                        execute!(
                                            stdout,
                                            cursor::MoveTo(0, win_rows),
                                            Print(utils::truncate_from_left(&current_line, prompt_len, win_cols, line_col)),
                                            cursor::MoveTo(cursor_col, win_rows),
                                        )?;
                                    }
                                } else {
                                    // simply move cursor and line position left
                                    execute!(
                                        stdout,
                                        cursor::MoveLeft(1),
                                    )?;
                                    cursor_col -= 1;
                                    line_col -= 1;
                                }
                            },
                            KeyCode::Right => {
                                if line_col == current_line.len() {
                                    // at the very end of the current typed line
                                    continue;
                                }
                                if cursor_col < (win_cols - 1) {
                                    // simply move cursor and line position right
                                    execute!(
                                        stdout,
                                        cursor::MoveRight(1),
                                    )?;
                                    cursor_col += 1;
                                    line_col += 1;
                                } else {
                                    // virtual scroll rightward through line
                                    line_col += 1;
                                    execute!(
                                        stdout,
                                        cursor::MoveTo(0, win_rows),
                                        Print(utils::truncate_from_right(&current_line, prompt_len, win_cols, line_col)),
                                    )?;
                                }
                            },
                            KeyCode::Enter => {
                                // if we were in search mode, pull command from that
                                let command = if !search_mode {
                                        current_line[prompt_len..].to_string()
                                    } else {
                                        command_history.search(
                                            &current_line[prompt_len..],
                                            search_depth
                                        ).unwrap_or(&current_line[prompt_len..]).to_string()
                                    };
                                let next = format!("{} > ", our.name);
                                execute!(
                                    stdout,
                                    cursor::MoveTo(0, win_rows),
                                    terminal::Clear(ClearType::CurrentLine),
                                    Print(&format!("{} > {}", our.name, command)),
                                    Print("\r\n"),
                                    Print(&next),
                                )?;
                                search_mode = false;
                                search_depth = 0;
                                current_line = next;
                                command_history.add(command.clone());
                                cursor_col = prompt_len.try_into().unwrap();
                                line_col = prompt_len;
                                event_loop.send(
                                    KernelMessage {
                                        id: rand::random(),
                                        source: Address {
                                            node: our.name.clone(),
                                            process: TERMINAL_PROCESS_ID.clone(),
                                        },
                                        target: Address {
                                            node: our.name.clone(),
                                            process: TERMINAL_PROCESS_ID.clone(),
                                        },
                                        rsvp: None,
                                        message: Message::Request(Request {
                                            inherit: false,
                                            expects_response: None,
                                            body: command.into_bytes(),
                                            metadata: None,
                                            capabilities: vec![],
                                        }),
                                        lazy_load_blob: None,
                                    }
                                ).await.expect("terminal: couldn't execute command!");
                            },
                            _ => {},
                        }
                    },
                    _ => {},
                }
            }
            _ = sigalrm.recv() => return Err(anyhow::anyhow!("exiting due to SIGALRM")),
            _ = sighup.recv() =>  return Err(anyhow::anyhow!("exiting due to SIGHUP")),
            _ = sigint.recv() =>  return Err(anyhow::anyhow!("exiting due to SIGINT")),
            _ = sigpipe.recv() => return Err(anyhow::anyhow!("exiting due to SIGPIPE")),
            _ = sigquit.recv() => return Err(anyhow::anyhow!("exiting due to SIGQUIT")),
            _ = sigterm.recv() => return Err(anyhow::anyhow!("exiting due to SIGTERM")),
            _ = sigusr1.recv() => return Err(anyhow::anyhow!("exiting due to SIGUSR1")),
            _ = sigusr2.recv() => return Err(anyhow::anyhow!("exiting due to SIGUSR2")),
        }
    }
    execute!(stdout.lock(), DisableBracketedPaste, terminal::SetTitle(""))?;
    Ok(())
}
