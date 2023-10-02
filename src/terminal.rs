use anyhow::Result;
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
use std::collections::VecDeque;
use std::fs::{read_to_string, File, OpenOptions};
use std::io::{stdout, BufWriter, Write};

use crate::types::*;

#[derive(Debug)]
struct CommandHistory {
    pub lines: VecDeque<String>,
    pub working_line: Option<String>,
    pub max_size: usize,
    pub index: usize,
    pub history_writer: BufWriter<File>,
}

impl CommandHistory {
    fn new(max_size: usize, history: String, history_writer: BufWriter<File>) -> Self {
        let mut lines = VecDeque::with_capacity(max_size);
        for line in history.lines() {
            lines.push_front(line.to_string());
        }
        Self {
            lines,
            working_line: None,
            max_size,
            index: 0,
            history_writer,
        }
    }

    fn add(&mut self, line: String) {
        self.working_line = None;
        // only add line to history if it's not exactly the same
        // as the previous line
        if &line != self.lines.front().unwrap_or(&"".into()) {
            let _ = writeln!(self.history_writer, "{}", &line);
            self.lines.push_front(line);
        }
        self.index = 0;
        if self.lines.len() > self.max_size {
            self.lines.pop_back();
        }
    }

    fn get_prev(&mut self, working_line: &str) -> Option<String> {
        if self.lines.len() == 0 || self.index == self.lines.len() {
            return None;
        }
        self.index += 1;
        if self.index == 1 {
            self.working_line = Some(working_line.into());
        }
        let line = self.lines[self.index - 1].clone();
        Some(line)
    }

    fn get_next(&mut self) -> Option<String> {
        if self.lines.len() == 0 || self.index == 0 || self.index == 1 {
            self.index = 0;
            if let Some(line) = self.working_line.clone() {
                self.working_line = None;
                return Some(line);
            }
            return None;
        }
        self.index -= 1;
        Some(self.lines[self.index - 1].clone())
    }

    /// if depth = 0, find most recent command in history that contains the
    /// provided string. otherwise, skip the first <depth> matches.
    /// yes this is O(n) to provide desired ordering, can revisit if slow
    fn search(&mut self, find: &str, depth: usize) -> Option<String> {
        let mut skips = 0;
        for line in &self.lines {
            if line.contains(find) && skips == depth {
                return Some(line.to_string());
            }
            skips += 1;
        }
        None
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
) -> Result<()> {
    let mut stdout = stdout();
    execute!(
        stdout,
        EnableBracketedPaste,
        terminal::SetTitle(format!("{}@{}", our.name, "uqbar"))
    )?;

    // print initial splash screen
    println!(
        "\x1b[38;5;128m{}\x1b[0m",
        format!(
            r#"

                ,,   UU
            s#  lUL  UU       !p
           !UU  lUL  UU       !UUlb
       #U  !UU  lUL  UU       !UUUUU#
       UU  !UU  lUL  UU       !UUUUUUUb
       UU  !UU  %"     ;-     !UUUUUUUU#
   $   UU  !UU         @UU#p  !UUUUUUUUU#
  ]U   UU  !#          @UUUUS !UUUUUUUUUUb
  @U   UU  !           @UUUUUUlUUUUUUUUUUU                         888
  UU   UU  !           @UUUUUUUUUUUUUUUUUU                         888
  @U   UU  !           @UUUUUU!UUUUUUUUUUU                         888
  'U   UU  !#          @UUUU# !UUUUUUUUUU~       888  888  .d88888 88888b.   8888b.  888d888
   \   UU  !UU         @UU#^  !UUUUUUUUU#        888  888 d88" 888 888 "88b     "88b 888P"
       UU  !UU  @Np  ,,"      !UUUUUUUU#         888  888 888  888 888  888 .d888888 888
       UU  !UU  lUL  UU       !UUUUUUU^          Y88b 888 Y88b 888 888 d88P 888  888 888
       "U  !UU  lUL  UU       !UUUUUf             "Y88888  "Y88888 88888P"  "Y888888 888
           !UU  lUL  UU       !UUl^                            888
            `"  lUL  UU       '^                               888    {}
                     ""                                        888    version {}

            "#,
            our.name, version
        )
    );

    enable_raw_mode()?;
    let mut reader = EventStream::new();
    let mut current_line = format!("{} > ", our.name);
    let prompt_len: usize = our.name.len() + 3;
    let (mut win_cols, mut win_rows) = terminal::size().unwrap();
    let mut cursor_col: u16 = prompt_len.try_into().unwrap();
    let mut line_col: usize = cursor_col as usize;
    let mut in_step_through: bool = false;
    // TODO add more verbosity levels as needed?
    // defaulting to TRUE for now, as we are BUIDLING
    // DEMO: default to false
    let mut verbose_mode: bool = false;
    let mut search_mode: bool = false;
    let mut search_depth: usize = 0;

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
    let mut command_history = CommandHistory::new(1000, history, history_writer);

    let log_path = std::fs::canonicalize(&home_directory_path)
        .unwrap()
        .join(".terminal_log");
    let log_handle = OpenOptions::new()
        .append(true)
        .create(true)
        .open(&log_path)
        .unwrap();
    let mut log_writer = BufWriter::new(log_handle);

    loop {
        let event = reader.next().fuse();

        tokio::select! {
            prints = print_rx.recv() => match prints {
                Some(printout) => {
                    let _ = writeln!(log_writer, "{}", printout.content);
                    if match printout.verbosity {
                        0 => false,
                        1 => !verbose_mode,
                        _ => true
                    } {
                        continue;
                    }
                    let mut stdout = stdout.lock();
                    execute!(
                        stdout,
                        cursor::MoveTo(0, win_rows - 1),
                        terminal::Clear(ClearType::CurrentLine)
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
                        Print(truncate_in_place(&current_line, prompt_len, win_cols, (line_col, cursor_col))),
                        cursor::MoveTo(cursor_col, win_rows),
                    )?;
                },
                None => {
                    write!(stdout.lock(), "terminal: lost print channel, crashing")?;
                    break;
                }
            },
            maybe_event = event => match maybe_event {
                Some(Ok(event)) => {
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
                                Print(truncate_in_place(&current_line, prompt_len, win_cols, (line_col, cursor_col))),
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
                            disable_raw_mode()?;
                            break;
                        },
                        // CTRL+V: toggle verbose mode
                        Event::Key(KeyEvent {
                            code: KeyCode::Char('v'),
                            modifiers: KeyModifiers::CONTROL,
                            ..
                        }) => {
                            let _ = print_tx.send(
                                Printout {
                                    verbosity: 0,
                                    content: match verbose_mode {
                                        true => "verbose mode off".into(),
                                        false => "verbose mode on".into(),
                                    }
                                }
                            ).await;
                            verbose_mode = !verbose_mode;
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
                                Print(truncate_rightward(&current_line, prompt_len, win_cols)),
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
                                Print(truncate_rightward(&current_line, prompt_len, win_cols)),
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
                                Print(truncate_from_left(&current_line, prompt_len, win_cols, line_col)),
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
                                Print(truncate_from_right(&current_line, prompt_len, win_cols, line_col)),
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
                            if let Some(result) = command_history.search(&current_line[prompt_len..], search_depth) {
                                // todo show search result with search query underlined
                                // and cursor in correct spot
                                execute!(
                                    stdout,
                                    cursor::MoveTo(0, win_rows),
                                    Print(truncate_in_place(
                                        &format!("{} * {}", our.name, result),
                                        prompt_len,
                                        win_cols,
                                        (line_col, cursor_col))),
                                    cursor::MoveTo(cursor_col, win_rows),
                                )?;
                            } else {
                                execute!(
                                    stdout,
                                    cursor::MoveTo(0, win_rows),
                                    Print(truncate_in_place(&current_line, prompt_len, win_cols, (line_col, cursor_col))),
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
                                Print(truncate_in_place(&current_line, prompt_len, win_cols, (line_col, cursor_col))),
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
                                        if let Some(result) = command_history.search(&current_line[prompt_len..], search_depth) {
                                            // todo show search result with search query underlined
                                            // and cursor in correct spot
                                            execute!(
                                                stdout,
                                                cursor::MoveTo(0, win_rows),
                                                Print(truncate_in_place(
                                                    &format!("{} * {}", our.name, result),
                                                    prompt_len,
                                                    win_cols,
                                                    (line_col, cursor_col))),
                                                cursor::MoveTo(cursor_col, win_rows),
                                            )?;
                                            continue
                                        }
                                    }
                                    execute!(
                                        stdout,
                                        cursor::MoveTo(0, win_rows),
                                        Print(truncate_in_place(&current_line, prompt_len, win_cols, (line_col, cursor_col))),
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
                                        if let Some(result) = command_history.search(&current_line[prompt_len..], search_depth) {
                                            // todo show search result with search query underlined
                                            // and cursor in correct spot
                                            execute!(
                                                stdout,
                                                cursor::MoveTo(0, win_rows),
                                                Print(truncate_in_place(
                                                    &format!("{} * {}", our.name, result),
                                                    prompt_len,
                                                    win_cols,
                                                    (line_col, cursor_col))),
                                                cursor::MoveTo(cursor_col, win_rows),
                                            )?;
                                            continue
                                        }
                                    }
                                    execute!(
                                        stdout,
                                        cursor::MoveTo(0, win_rows),
                                        terminal::Clear(ClearType::CurrentLine),
                                        Print(truncate_in_place(&current_line, prompt_len, win_cols, (line_col, cursor_col))),
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
                                                Print(truncate_from_left(&current_line, prompt_len, win_cols, line_col)),
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
                                            Print(truncate_from_right(&current_line, prompt_len, win_cols, line_col)),
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
                                            ).unwrap_or(current_line[prompt_len..].to_string())
                                        };
                                    let next = format!("{} > ", our.name);
                                    execute!(
                                        stdout,
                                        cursor::MoveTo(0, win_rows),
                                        terminal::Clear(ClearType::CurrentLine),
                                        Print(&current_line),
                                        Print("\r\n"),
                                        Print(&next),
                                    )?;
                                    search_mode = false;
                                    search_depth = 0;
                                    current_line = next;
                                    command_history.add(command.clone());
                                    cursor_col = prompt_len.try_into().unwrap();
                                    line_col = prompt_len;
                                    let _err = event_loop.send(
                                        KernelMessage {
                                            id: rand::random(),
                                            source: Address {
                                                node: our.name.clone(),
                                                process: ProcessId::Name("terminal".into()),
                                            },
                                            target: Address {
                                                node: our.name.clone(),
                                                process: ProcessId::Name("terminal".into()),
                                            },
                                            rsvp: None,
                                            message: Message::Request(Request {
                                                inherit: false,
                                                expects_response: None,
                                                ipc: Some(command),
                                                metadata: None,
                                            }),
                                            payload: None,
                                            signed_capabilities: None,
                                        }
                                    ).await;
                                },
                                _ => {},
                            }
                        },
                        _ => {},
                    }
                }
                Some(Err(e)) => println!("Error: {:?}\r", e),
                None => break,
            }
        }
    }
    execute!(stdout.lock(), DisableBracketedPaste, terminal::SetTitle(""))?;
    disable_raw_mode()?;
    Ok(())
}

fn truncate_rightward(s: &str, prompt_len: usize, width: u16) -> String {
    if s.len() <= width as usize {
        // no adjustment to be made
        return s.to_string();
    }
    let sans_prompt = &s[prompt_len..];
    s[..prompt_len].to_string() + &sans_prompt[(s.len() - width as usize)..]
}

/// print prompt, then as many chars as will fit in term starting from line_col
fn truncate_from_left(s: &str, prompt_len: usize, width: u16, line_col: usize) -> String {
    if s.len() <= width as usize {
        // no adjustment to be made
        return s.to_string();
    }
    s[..prompt_len].to_string() + &s[line_col..(width as usize - prompt_len + line_col)]
}

/// print prompt, then as many chars as will fit in term leading up to line_col
fn truncate_from_right(s: &str, prompt_len: usize, width: u16, line_col: usize) -> String {
    if s.len() <= width as usize {
        // no adjustment to be made
        return s.to_string();
    }
    s[..prompt_len].to_string() + &s[(prompt_len + (line_col - width as usize))..line_col]
}

/// if line is wider than the terminal, truncate it intelligently,
/// keeping the cursor in the same relative position.
fn truncate_in_place(
    s: &str,
    prompt_len: usize,
    width: u16,
    (line_col, cursor_col): (usize, u16),
) -> String {
    if s.len() <= width as usize {
        // no adjustment to be made
        return s.to_string();
    }
    // always keep prompt at left
    let prompt = &s[..prompt_len];
    // print as much of the command fits left of col_in_command before cursor_col,
    // then fill out the rest up to width
    let end = width as usize + line_col - cursor_col as usize;
    if end > s.len() {
        return s.to_string();
    }
    prompt.to_string() + &s[(prompt_len + line_col - cursor_col as usize)..end]
}
