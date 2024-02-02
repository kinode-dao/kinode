use std::collections::VecDeque;
use std::fs::File;
use std::io::{BufWriter, Write};

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

pub fn underline(s: &str, to_underline: &str) -> String {
    // format result string to have query portion underlined
    let mut result = s.to_string();
    let u_start = s.find(to_underline).unwrap();
    let u_end = u_start + to_underline.len();
    result.insert_str(u_end, "\x1b[24m");
    result.insert_str(u_start, "\x1b[4m");
    result
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
