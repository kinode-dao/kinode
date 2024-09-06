use crossterm::terminal::{disable_raw_mode, enable_raw_mode};
use lib::types::core::Identity;
use std::{
    collections::VecDeque,
    fs::File,
    io::{BufWriter, Stdout, Write},
};
use unicode_segmentation::UnicodeSegmentation;

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

/// produce command line prompt and its length
pub fn make_prompt(our_name: &str) -> (&'static str, usize) {
    let prompt = Box::leak(format!("{} > ", our_name).into_boxed_str());
    (
        prompt,
        UnicodeSegmentation::graphemes(prompt, true)
            .collect::<Vec<_>>()
            .len(),
    )
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

pub fn underline(s: &str, to_underline: &str) -> (String, u16) {
    // format result string to have query portion underlined
    let mut result = s.to_string();
    let u_start = s.find(to_underline).unwrap();
    let u_end = u_start + to_underline.graphemes(true).count();
    result.insert_str(u_end, "\x1b[24m");
    result.insert_str(u_start, "\x1b[4m");
    (result, u_end as u16)
}

/// if line is wider than the terminal, truncate it intelligently,
/// keeping the cursor in the same relative position.
pub fn truncate_in_place(s: &str, term_width: usize, line_col: usize, cursor_col: u16) -> String {
    let graphemes_count = s.graphemes(true).count();
    if graphemes_count <= term_width {
        // no adjustment to be made
        return s.to_string();
    }

    // input line is wider than terminal, clip start/end/both while keeping cursor
    // in same relative position.
    if (cursor_col as usize) == line_col {
        // beginning of line is placed at left end, truncate everything past term_width
        s.graphemes(true).take(term_width).collect::<String>()
    } else if (cursor_col as usize) < line_col {
        // some amount of the line is to the left of the terminal, clip from the right
        s.graphemes(true)
            .skip(line_col - cursor_col as usize)
            .take(term_width)
            .collect::<String>()
    } else {
        // this cannot occur
        unreachable!()
    }
}
