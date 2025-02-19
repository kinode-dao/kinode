use crossterm::terminal::{disable_raw_mode, enable_raw_mode};
use lib::types::core::Identity;
use std::{
    collections::VecDeque,
    fs::{File, OpenOptions},
    io::{BufWriter, Stdout, Write},
    path::{Path, PathBuf},
};
use unicode_segmentation::UnicodeSegmentation;
use unicode_width::UnicodeWidthStr;

const DEFAULT_MAX_LOGS_BYTES: u64 = 16_000_000;
const DEFAULT_NUMBER_LOG_FILES: u64 = 4;

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
    our_ip: &std::net::Ipv4Addr,
    home_directory_path: &Path,
) -> std::io::Result<(Stdout, Option<RawMode>)> {
    let mut stdout = std::io::stdout();
    crossterm::execute!(
        stdout,
        crossterm::event::EnableBracketedPaste,
        crossterm::terminal::SetTitle(format!("hyperdrive {}", our.name))
    )?;

    let (win_cols, _) = crossterm::terminal::size().unwrap_or_else(|_| (0, 0));

    // print initial splash screen, large if there's room, small otherwise
    if win_cols >= 119 {
        crossterm::execute!(
            stdout,
            crossterm::style::SetForegroundColor(crossterm::style::Color::Magenta),
            crossterm::style::Print(format!(r#"
   ▄█    █▄    ▄██   ▄      ▄███████▄    ▄████████    ▄███████▄  ████████▄     ▄███████▄   ▄█   ▄█    █▄     ▄████████
  ███    ███   ███   ██▄   ███    ███   ███    ███   ███    ███  ███   ▀███   ███    ███  ███  ███    ███   ███    ███
  ███    ███   ███▄▄▄███   ███    ███   ███    █▀    ███    ███  ███    ███   ███    ███  ███▌ ███    ███   ███    █▀
 ▄███▄▄▄▄███▄▄ ▀▀▀▀▀▀███   ███    ███  ▄███▄▄▄      ▄███▄▄▄▄██▀  ███    ███  ▄███▄▄▄▄██▀  ███▌ ███    ███  ▄███▄▄▄
▀▀███▀▀▀▀███▀  ▄██   ███ ▀█████████▀  ▀▀███▀▀▀     ▀██████████   ███    ███ ▀██████████▄  ███▌ ███    ███ ▀▀███▀▀▀
  ███    ███   ███   ███   ███          ███    █▄    ███    ███  ███    ███   ███    ███  ███   ███   ███   ███    █▄
  ███    ███   ███   ███   ███          ███    ███   ███    ███  ███   ▄███   ███    ███  ███    ███  ███   ███    ███
  ███    █▀     ▀█████▀   ▄████▀        ██████████   ███     ▀█  ████████▀    ███     ▀█  █▀      ▀████▀    ██████████

@@@@@@@@@@@=:#@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@%@@@@@@@@@
@@@@@@@@@@@@%@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@
@@@@@@@@@@@@@@@@@@@@%@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@
@@@@@@@@@@@@@@@@@@@@%@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@
@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@
@@@@@@@@@@@@@@@%@@@@@@@@@@@@@@@@@@@@@@@@%%**++=========++*#%%@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@
@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@%*=------------------------=+#@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@
@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@%*----------------------------------#@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@
@@@@@@@@@@@@@@@@@@@@@@@@@@@%#=----------------------------------=----*%@@@@@@@@@@@@@@@@@@@@@@@@@@@@@%@@@@@@@@@@@@@@@@@
@@@@@@@@@@@@@@@@@@@@@@@@@%+---------------------------------------===--+%@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@
@@@@@@@@@@@@@@@@@@@@@@@#=-------------------------------------------=++-=#@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@
@@@@@@@@@@@@@@@%%@@@@%=---------------------------------------------=+++--+%@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@
@@@@@@@@@@@@@@@@@@@@#------------------------------------------------=+**=-=%@@@@@@@@@@@@@@@@@@@@@@@@@@+*@@@@@@@@@@@@@
@@@@@@@@@@@@@@@@@@@*------------------------------------------------=======--+%@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@
@@@@@@@@@@@@@@@@@@*-----------------------------------------------===+++++==+=-*%@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@%=%@@@@
@@@@@@@@@@@@@@@@@#-----------------------------------------------=======--------=#@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@
@@@@@@@@@@@@@@@@%=----------------------------------------------+##--------------%@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@
@@@@@@@@@@@@@@@@#----------------------------------------------+=++*=--=====+---*%@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@
@@@@@@@@@@@@@@@@=---------------------------------------=+=-===+-+=#*+====+=---+%@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@
@@@@@@@@@@@@@@@%--------------------------------------=+=====--+-+-=#+=--------*@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@
@@@@@@@@@@@@@@@%---------------------------------------=**++===+=*=-*+==-------*@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@
@@@@@@@@@@@@@@@%-----------------------------------------=+**++#=-*-+*==--------+%@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@
@@@@@@@@@@@@@@@@=-----------------------------------=+=---===+#++=++++++===-------#@@@@@@#*@@@@@@@@@@@@@@@@@@@@@@%@@@@
@@@@@@@@@@@@@@@@+-----------------------------------=====----=#=#*=++*+==+====-----#@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@
@@@@@@@@@@@@@@@@#=-----------------------------------+*+=====-*%###+#%+=======-----=#@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@
@@@@@@@@@@@@@@@@%==-----------------------------=*+==-------=**#*%@%**=-===+++=-----+%%@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@
@@@@@@@@@@@@@@@@@*-+---=+-----=------+=-----=*=------=+*+=----=##+=+#+++-==++===----=*=%@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@
@@@@@@@@@@@@@@+*@%+--=+----==----=+=----=++----==++=--=++*+=--=++**+****=========---=#=-#@@@@@@@@@@@@@@@@@@@@@@@@@@@@@
@@@@@@@@@@@@@@@@@@*++---==----=+=---=++=---=+++---+*++==+*+++=-=+++++++==+++=====----#*#@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@
@@@@@@@@@@@@@@@@@@@+--+=----+=---=*+=--=**=---+*+=-=*#+=-----===+=+****---+*+++++=---*%@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@
@@@@@@@@@@@@@@@@@@@%++---=*=--=*+---=*+---=*#=-=*#+--+#*=++---*+*=----==----+*++++---=%@@@@@@@@@@@@@@@@@@%@@@@@@@@@@@@
@#:+%@@@@@@@@@@@@@@@%=-++---+*---=*+---+#*==+#+==+#*++#%@=-----+*#+=---#*+=----+*+----%@@@@@@@@@@@@@@@@@%@@@@@@@@@@@@@
@@%%@@@@%%@@@@@@@@@@@%*=-=++--=*+--=+*+=+**+-=**+=*#*=+*=-------=#+++==++==-----=+----#@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@
@@@@@@@%=+@@@@@@@@@@@@%**=-=++=-=**=-+*++*+++++====++=-+*+=------+*+++=-*+=-----------=@@@@@@@@@@@@@@@@@%@@@@@@@@@@@@@
@@@@@@@@@@@@@@@@@@@@@@@@*=#=--+*=-+#+=++=-----------**+--=**=----=%---=-+*=------------%@@@@@%@@@@@@@@@@@@@@@@@@@@@@@@
@@@@@@@@@@@@@@@@@@@@@@@@@%-=*+--**--*+-=*=----------=++#+---+-----=#-----*+------------*@@@@*-%@@@@@@@@@@@@@@@@@@@@@@@
@@@@@@@@@@@@@@@@@@@@@@@@@@%+-+*=-+#-=+=-=++----------+++**=-===----#+=---*=------------=%@@@@@@@@@@@@@@@@@@@@@@@@@@@@@
@@@@@@@@@@@@@@@@@@@@@@@@@@@%*-====+=-=+=---=----------++=+++=-=----+*+=--+=-------------#@@@@@@@@@@@@@@@@@@@@@@@@@@@@@
@@@@%%@@@@@@@@@@@@@@@@@@@@@@*=+++-+=+--++=-------------*=-+*+=------#+=--++-------------+@@@@@@@@@@@@@@@@@@@@@@@@@@@@@
@@@@@@@@@@@@@@@@@@@@@@@@@@@@@*==***-=+=--++=--------=--=%=-=**=-----+*++-+#+=-----------=#@@@@@@@%@@@@@@@@@@@@@@@@@@@@
@@@@@@@@@@@@@@@@@@@@@@@@@@@@%*%+-+#*--++----=-------==---#*----+=---=*====*#*=-----------=%@@@@@@@@@@@@@@@@@@@@@@@@@@@
@@@@@@@@@@@@@@@@@@@@@@@@@@@@@*=*+=+=+=-=+*=---------+=---=++*+-------*+---=#***==+-=------*@@@@@@@@@@@@@@@@@@@@@@@%%@@
@@@@@@@@@@@@@@@@@@@@@@@@@@@@@#+=+**+-++=-===--------===---*==++=-----+*+----+#%%%*==+==+=-*@@@@@@@@@@@@@@@@@@@@@@@**@@
@@@%@@@@@@@@@*=@@@@@@@@@@@@@%#*=-=**=-++--==---------=+=--=*=-++=-----#+-----=+=+*+=*+=++*%@@@@@@@@@@@@@@@@@@@@@@@@@@@
@@@@@@@@@@@@@%%@@@@@@@@@@@@@%=-+**#=+=-=+=-----------===---+*=--++=---*+------+*+##=+#+=%@@@@@@@@@@@@@@@@@@@@@@@@@@@@@
@@@@@@@@@@@@@@@@@@@@@@@@@@@@##+--+*====----==---------------#*=--=+---=#=-----+%%%@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@
@@@@@@@@@@@@@@@@@@@@@@@@@@@%*-=+*#*+=--===---=---------------%*+===----#+==--=@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@

 {} ({})
 runtime version {}
 a general purpose sovereign cloud computer
 public IP {}
 home directory at {}
 networking pubkey {}{}
"#,
                our.name,
                if our.is_direct() {
                    "direct"
                } else {
                    "indirect"
                },
                version,
                our_ip,
                home_directory_path.display(),
                our.networking_key,
                if is_detached { "\n(detached)" } else { "" }
            )),
            crossterm::style::ResetColor
        )
        .expect("terminal: couldn't print splash");
    } else {
        crossterm::execute!(
            stdout,
            crossterm::style::SetForegroundColor(crossterm::style::Color::Magenta),
            crossterm::style::Print(format!(r#"
   ▄█    █▄    ▄██   ▄      ▄███████▄    ▄████████    ▄███████▄
  ███    ███   ███   ██▄   ███    ███   ███    ███   ███    ███
  ███    ███   ███▄▄▄███   ███    ███   ███    █▀    ███    ███
 ▄███▄▄▄▄███▄▄ ▀▀▀▀▀▀███   ███    ███  ▄███▄▄▄      ▄███▄▄▄▄██▀
▀▀███▀▀▀▀███▀  ▄██   ███ ▀█████████▀  ▀▀███▀▀▀     ▀██████████
  ███    ███   ███   ███   ███          ███    █▄    ███    ███
  ███    ███   ███   ███   ███          ███    ███   ███    ███
  ███    █▀     ▀█████▀   ▄████▀        ██████████   ███     ▀█

    ████████▄     ▄███████▄   ▄█   ▄█    █▄     ▄████████
    ███   ▀███   ███    ███  ███  ███    ███   ███    ███
    ███    ███   ███    ███  ███▌ ███    ███   ███    █▀
    ███    ███  ▄███▄▄▄▄██▀  ███▌ ███    ███  ▄███▄▄▄
    ███    ███ ▀██████████▄  ███▌ ███    ███ ▀▀███▀▀▀
    ███    ███   ███    ███  ███   ███   ███   ███    █▄
    ███   ▄███   ███    ███  ███    ███  ███   ███    ███
    ████████▀    ███     ▀█  █▀      ▀████▀    ██████████

 {} ({})
 version {}
 a general purpose sovereign cloud computer
 public IP {}
 home dir at {}
 net pubkey: {}{}
"#,
                our.name,
                if our.is_direct() {
                    "direct"
                } else {
                    "indirect"
                },
                version,
                our_ip,
                home_directory_path.display(),
                our.networking_key,
                if is_detached { "\n(detached)" } else { "" }
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

pub fn display_width(s: &str) -> usize {
    UnicodeWidthStr::width(s)
}

/// produce command line prompt and its length
pub fn make_prompt(our_name: &str) -> (&'static str, usize) {
    let prompt = Box::leak(format!("{} > ", our_name).into_boxed_str());
    (prompt, display_width(prompt))
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
    let mut u_end = u_start + to_underline.len();
    result.insert_str(u_end, "\x1b[24m");
    result.insert_str(u_start, "\x1b[4m");
    // check if u_end is at a character boundary
    loop {
        if result.is_char_boundary(u_end) {
            break;
        }
        u_end += 1;
    }
    let cursor_end = display_width(&result[..u_end]);
    (result, cursor_end as u16)
}

/// if line is wider than the terminal, truncate it intelligently,
/// keeping the cursor in the same relative position.
pub fn truncate_in_place(
    s: &str,
    term_width: u16,
    line_col: usize,
    cursor_col: u16,
    show_end: bool,
) -> String {
    let width = display_width(s);
    if width <= term_width as usize {
        // no adjustment to be made
        return s.to_string();
    }

    let graphemes_with_width = s.graphemes(true).map(|g| (g, display_width(g)));

    let adjusted_cursor_col = graphemes_with_width
        .clone()
        .take(cursor_col as usize)
        .map(|(_, w)| w)
        .sum::<usize>();

    // input line is wider than terminal, clip start/end/both while keeping cursor
    // in same relative position.
    if show_end || cursor_col >= term_width {
        // show end of line, truncate everything before
        let mut width = 0;
        graphemes_with_width
            .rev()
            .take_while(|(_, w)| {
                width += w;
                width <= term_width as usize
            })
            .map(|(g, _)| g)
            .collect::<String>()
            .chars()
            .rev()
            .collect::<String>()
    } else if adjusted_cursor_col as usize == line_col {
        // beginning of line is placed at left end, truncate everything past term_width
        let mut width = 0;
        graphemes_with_width
            .take_while(|(_, w)| {
                width += w;
                width <= term_width as usize
            })
            .map(|(g, _)| g)
            .collect::<String>()
    } else if adjusted_cursor_col < line_col {
        // some amount of the line is to the left of the terminal, clip from the right
        // skip the difference between line_col and cursor_col *after adjusting for
        // wide characters
        let mut width = 0;
        graphemes_with_width
            .skip(line_col - adjusted_cursor_col)
            .take_while(|(_, w)| {
                width += w;
                width <= term_width as usize
            })
            .map(|(g, _)| g)
            .collect::<String>()
    } else {
        // show end of line, truncate everything before
        let mut width = 0;
        graphemes_with_width
            .rev()
            .take_while(|(_, w)| {
                width += w;
                width <= term_width as usize
            })
            .map(|(g, _)| g)
            .collect::<String>()
            .chars()
            .rev()
            .collect::<String>()
    }
}

pub struct Logger {
    pub log_dir_path: PathBuf,
    pub strategy: LoggerStrategy,
    log_writer: BufWriter<std::fs::File>,
}

pub enum LoggerStrategy {
    Rotating {
        max_log_dir_bytes: u64,
        number_log_files: u64,
    },
    Infinite,
}

impl LoggerStrategy {
    fn new(max_log_size: Option<u64>, number_log_files: Option<u64>) -> Self {
        let max_log_size = max_log_size.unwrap_or_else(|| DEFAULT_MAX_LOGS_BYTES);
        let number_log_files = number_log_files.unwrap_or_else(|| DEFAULT_NUMBER_LOG_FILES);
        if max_log_size == 0 {
            LoggerStrategy::Infinite
        } else {
            LoggerStrategy::Rotating {
                max_log_dir_bytes: max_log_size,
                number_log_files,
            }
        }
    }
}

impl Logger {
    pub fn new(
        log_dir_path: PathBuf,
        max_log_size: Option<u64>,
        number_log_files: Option<u64>,
    ) -> Self {
        let log_writer = make_log_writer(&log_dir_path).unwrap();
        Self {
            log_dir_path,
            log_writer,
            strategy: LoggerStrategy::new(max_log_size, number_log_files),
        }
    }

    pub fn write(&mut self, line: &str) -> anyhow::Result<()> {
        let now = chrono::Local::now();
        let line = &format!("[{}] {}", now.to_rfc2822(), line);
        match self.strategy {
            LoggerStrategy::Infinite => {}
            LoggerStrategy::Rotating {
                max_log_dir_bytes,
                number_log_files,
            } => {
                // check whether to rotate
                let line_bytes = line.len();
                let file_bytes = self.log_writer.get_ref().metadata()?.len() as usize;
                if line_bytes + file_bytes >= (max_log_dir_bytes / number_log_files) as usize {
                    // rotate
                    self.log_writer = make_log_writer(&self.log_dir_path)?;

                    // clean up oldest if necessary
                    remove_oldest_if_exceeds(&self.log_dir_path, number_log_files as usize)?;
                }
            }
        }

        writeln!(self.log_writer, "{}", line)?;

        Ok(())
    }
}

fn make_log_writer(log_dir_path: &Path) -> anyhow::Result<BufWriter<std::fs::File>> {
    if !log_dir_path.exists() {
        std::fs::create_dir(log_dir_path)?;
    }
    let now = chrono::Local::now();
    #[cfg(unix)]
    let log_name = format!("{}.log", now.format("%Y-%m-%d-%H:%M:%S"));
    #[cfg(target_os = "windows")]
    let log_name = format!("{}.log", now.format("%Y-%m-%d-%H_%M_%S"));

    let log_path = log_dir_path.join(log_name);
    let log_handle = OpenOptions::new()
        .append(true)
        .create(true)
        .open(&log_path)?;
    Ok(BufWriter::new(log_handle))
}

fn remove_oldest_if_exceeds<P: AsRef<Path>>(path: P, max_items: usize) -> anyhow::Result<()> {
    let mut entries = Vec::new();

    // Collect all entries and their modification times
    for entry in std::fs::read_dir(path)? {
        let entry = entry?;
        if let Ok(metadata) = entry.metadata() {
            if let Ok(modified) = metadata.modified() {
                entries.push((modified, entry.path()));
            }
        }
    }

    // If the number of entries exceeds the max_items, remove the oldest
    while entries.len() > max_items {
        // Sort entries by modification time (oldest first)
        entries.sort_by_key(|e| e.0);

        let (_, path) = entries.remove(0);
        std::fs::remove_file(&path)?;
    }

    Ok(())
}
