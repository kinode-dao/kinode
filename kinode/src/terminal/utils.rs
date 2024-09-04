use crossterm::terminal::{disable_raw_mode, enable_raw_mode};
use lib::types::core::Identity;
use std::{
    collections::VecDeque,
    fs::File,
    io::{BufWriter, Stdout, Write},
};

pub struct RawMode;
impl RawMode {
    fn new() -> std::io::Result<Self> {
        enable_raw_mode()?;
        Ok(RawMode)
    }
}
impl Drop for RawMode {
    fn drop(&mut self) {
        match disable_raw_mode() {
            Ok(_) => {}
            Err(e) => {
                println!("terminal: failed to disable raw mode: {e:?}\r");
            }
        }
    }
}

pub fn splash(
    our: &Identity,
    version: &str,
    is_detached: bool,
) -> std::io::Result<(Stdout, Option<RawMode>)> {
    let mut stdout = std::io::stdout();
    crossterm::execute!(
        stdout,
        crossterm::event::EnableBracketedPaste,
        crossterm::terminal::SetTitle(format!("kinode {}", our.name))
    )?;

    let (win_cols, _) = crossterm::terminal::size().expect("terminal: couldn't fetch size");

    // print initial splash screen, large if there's room, small otherwise
    if win_cols >= 90 {
        crossterm::execute!(
            stdout,
            crossterm::style::SetForegroundColor(crossterm::style::Color::Magenta),
            crossterm::style::Print(format!(
                r#"
     .`
 `@@,,                     ,*    888    d8P  d8b                        888
   `@%@@@,            ,~-##`     888   d8P   Y8P                        888
     ~@@#@%#@@,      #####       888  d8P                               888
       ~-%######@@@, #####       888d88K     888 88888b.   .d88b.   .d88888  .d88b.
          -%%#######@#####,      8888888b    888 888 "88b d88""88b d88" 888 d8P  Y8b
            ~^^%##########@      888  Y88b   888 888  888 888  888 888  888 88888888
               >^#########@      888   Y88b  888 888  888 Y88..88P Y88b 888 Y8b.
                 `>#######`      888    Y88b 888 888  888  "Y88P"   "Y88888  "Y8888
                .>######%
               /###%^#%          {} ({})
             /##%@#  `           runtime version {}
          ./######`              a general purpose sovereign cloud computer
        /.^`.#^#^`
       `   ,#`#`#,
          ,/ /` `
        .*`
 networking public key: {}
 {}
                    "#,
                our.name,
                if our.is_direct() {
                    "direct"
                } else {
                    "indirect"
                },
                version,
                our.networking_key,
                if is_detached { "(detached)" } else { "" }
            )),
            crossterm::style::ResetColor
        )
        .expect("terminal: couldn't print splash");
    } else {
        crossterm::execute!(
            stdout,
            crossterm::style::SetForegroundColor(crossterm::style::Color::Magenta),
            crossterm::style::Print(format!(
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
 {}
                    "#,
                our.name,
                if our.is_direct() {
                    "direct"
                } else {
                    "indirect"
                },
                version,
                our.networking_key,
                if is_detached { "(detached)" } else { "" }
            )),
            crossterm::style::ResetColor
        )?;
    }

    Ok((
        stdout,
        if is_detached {
            None
        } else {
            Some(RawMode::new()?)
        },
    ))
}

pub fn cleanup(quit_msg: &str) {
    let stdout = std::io::stdout();
    let mut stdout = stdout.lock();
    crossterm::execute!(
        stdout,
        crossterm::event::DisableBracketedPaste,
        crossterm::terminal::SetTitle(""),
        crossterm::style::SetForegroundColor(crossterm::style::Color::Red),
        crossterm::style::Print(format!("\r\n{quit_msg}\r\n")),
        crossterm::style::ResetColor,
    )
    .expect("failed to clean up terminal visual state! your terminal window might be funky now");
}

#[derive(Debug)]
pub struct CommandHistory {
    lines: VecDeque<String>,
    working_line: Option<String>,
    max_size: usize,
    index: usize,
    history_writer: BufWriter<File>,
}

impl CommandHistory {
    pub fn new(max_size: usize, history: String, history_writer: BufWriter<File>) -> Self {
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

    pub fn add(&mut self, line: String) {
        self.working_line = None;
        // only add line to history if it's not exactly the same
        // as the previous line and also not an empty line
        if &line != self.lines.front().unwrap_or(&"".into()) && line != "" {
            let _ = writeln!(self.history_writer, "{}", &line);
            self.lines.push_front(line);
        }
        self.index = 0;
        if self.lines.len() > self.max_size {
            self.lines.pop_back();
        }
    }

    pub fn get_prev(&mut self, working_line: &str) -> Option<String> {
        if self.lines.is_empty() || self.index == self.lines.len() {
            return None;
        }
        self.index += 1;
        if self.index == 1 {
            self.working_line = Some(working_line.into());
        }
        let line = self.lines[self.index - 1].clone();
        Some(line)
    }

    pub fn get_next(&mut self) -> Option<String> {
        if self.lines.is_empty() || self.index == 0 || self.index == 1 {
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
    pub fn search(&mut self, find: &str, depth: usize) -> Option<&str> {
        let mut skips = 0;
        if find.is_empty() {
            return None;
        }
        // if there is at least one match, and we've skipped past it, return oldest match
        let mut last_match: Option<&str> = None;
        for line in self.lines.iter() {
            if line.contains(find) {
                last_match = Some(line);
                if skips == depth {
                    return Some(line);
                } else {
                    skips += 1;
                }
            }
        }
        last_match
    }
}

pub fn execute_search(
    our: &Identity,
    stdout: &mut std::io::StdoutLock,
    current_line: &str,
    prompt_len: usize,
    (win_cols, win_rows): (u16, u16),
    (line_col, cursor_col): (usize, u16),
    command_history: &mut CommandHistory,
    search_depth: usize,
) -> Result<(), std::io::Error> {
    let search_query = &current_line[prompt_len..];
    if let Some(result) = command_history.search(search_query, search_depth) {
        let (result_underlined, u_end) = underline(result, search_query);
        let search_cursor_col = u_end + prompt_len as u16;
        crossterm::execute!(
            stdout,
            crossterm::cursor::MoveTo(0, win_rows),
            crossterm::terminal::Clear(crossterm::terminal::ClearType::CurrentLine),
            crossterm::style::Print(truncate_in_place(
                &format!("{} * {}", our.name, result_underlined),
                prompt_len,
                win_cols,
                (line_col, search_cursor_col)
            )),
            crossterm::cursor::MoveTo(search_cursor_col, win_rows),
        )
    } else {
        crossterm::execute!(
            stdout,
            crossterm::cursor::MoveTo(0, win_rows),
            crossterm::terminal::Clear(crossterm::terminal::ClearType::CurrentLine),
            crossterm::style::Print(truncate_in_place(
                &format!("{} * {}: no results", our.name, &current_line[prompt_len..]),
                prompt_len,
                win_cols,
                (line_col, cursor_col)
            )),
            crossterm::cursor::MoveTo(cursor_col, win_rows),
        )
    }
}

pub fn underline(s: &str, to_underline: &str) -> (String, u16) {
    // format result string to have query portion underlined
    let mut result = s.to_string();
    let u_start = s.find(to_underline).unwrap();
    let u_end = u_start + to_underline.len();
    result.insert_str(u_end, "\x1b[24m");
    result.insert_str(u_start, "\x1b[4m");
    (result, u_end as u16)
}

pub fn truncate_rightward(s: &str, prompt_len: usize, width: u16) -> String {
    if s.len() <= width as usize {
        // no adjustment to be made
        return s.to_string();
    }
    let sans_prompt = &s[prompt_len..];
    s[..prompt_len].to_string() + &sans_prompt[(s.len() - width as usize)..]
}

/// print prompt, then as many chars as will fit in term starting from line_col
pub fn truncate_from_left(s: &str, prompt_len: usize, width: u16, line_col: usize) -> String {
    if s.len() <= width as usize {
        // no adjustment to be made
        return s.to_string();
    }
    s[..prompt_len].to_string() + &s[line_col..(width as usize - prompt_len + line_col)]
}

/// print prompt, then as many chars as will fit in term leading up to line_col
pub fn truncate_from_right(s: &str, prompt_len: usize, width: u16, line_col: usize) -> String {
    if s.len() <= width as usize {
        // no adjustment to be made
        return s.to_string();
    }
    s[..prompt_len].to_string() + &s[(prompt_len + (line_col - width as usize))..line_col]
}

/// if line is wider than the terminal, truncate it intelligently,
/// keeping the cursor in the same relative position.
pub fn truncate_in_place(
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
