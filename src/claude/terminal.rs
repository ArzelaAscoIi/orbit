//! One program on a pseudo-terminal (a `claude attach` client, or a shell
//! in a pane), its VT100 screen, and how egui draws it and feeds it the
//! keyboard.

use std::io::{Read, Write};
use std::path::Path;
use std::sync::{Arc, Mutex, PoisonError};
use std::time::{Duration, Instant};

use anyhow::Result;
use egui::{Color32, FontId, Rect, Sense, Ui, pos2, vec2};
use portable_pty::{Child, CommandBuilder, MasterPty, PtySize, native_pty_system};

use super::shell;
use crate::theme;

/// The terminal title the program set (OSC 0/2): Claude Code puts its
/// spinner there while it works (screen.rs reads it).
#[derive(Default)]
pub struct Title(String);

impl vt100::Callbacks for Title {
    fn set_window_title(&mut self, _: &mut vt100::Screen, title: &[u8]) {
        self.0 = String::from_utf8_lossy(title).into_owned();
    }
}

/// Dropping it only detaches; the background session keeps running.
pub struct Terminal {
    screen: Arc<Mutex<vt100::Parser<Title>>>,
    writer: Box<dyn Write + Send>,
    master: Box<dyn MasterPty + Send>,
    child: Box<dyn Child + Send + Sync>,
    size: (u16, u16),
    /// Cell edges the user dragged between (anchor, end; col 0..=cols) on the visible screen;
    /// Cmd+C copies them.
    selection: Option<((u16, u16), (u16, u16))>,
    /// Scroll not yet a whole line: a trackpad sends a few points a frame.
    scroll_rest: f32,
    exited: bool,
    spawned: Instant,
    /// The last moment the screen showed work (screen.rs): the prompt box
    /// alone counts as idle only once that is a second ago, so a status
    /// line that blinks between two tool calls does not read as a turn
    /// that ended (herdr holds its idle the same way).
    working_at: Option<Instant>,
}

impl Terminal {
    pub(super) fn spawn(cwd: &Path, args: &[String], ctx: &egui::Context) -> Result<Self> {
        Self::spawn_program(shell::claude_path(), cwd, args, ctx)
    }

    /// The user's login shell, for a pane: k9s, logs, anything.
    pub fn shell(ctx: &egui::Context) -> Result<Self> {
        Self::shell_in(None, ctx)
    }

    /// A login shell in `dir` (a split beside another shell), else home.
    pub fn shell_in(dir: Option<&Path>, ctx: &egui::Context) -> Result<Self> {
        let home = directories::BaseDirs::new()
            .map(|d| d.home_dir().to_path_buf())
            .unwrap_or_else(|| "/".into());
        let dir = dir.filter(|d| d.is_dir()).unwrap_or(&home);
        Self::spawn_program(Path::new(&shell::shell()), dir, &["-l".into()], ctx)
    }

    /// Where the shell is now: the working folder of what runs in front
    /// (the shell after its last `cd`, or k9s), so a split opens beside it.
    pub fn cwd(&self) -> Option<std::path::PathBuf> {
        cwd_of(self.cwd_pid()?)
    }

    /// The process whose folder is the terminal's: the foreground job, else
    /// the shell. Cheap; `cwd_of` asks lsof, so callers on the UI thread
    /// hand the pid to a thread.
    pub fn cwd_pid(&self) -> Option<u32> {
        self.master
            .process_group_leader()
            .and_then(|p| u32::try_from(p).ok())
            .or_else(|| self.child.process_id())
    }

    fn spawn_program(
        program: &Path,
        cwd: &Path,
        args: &[String],
        ctx: &egui::Context,
    ) -> Result<Self> {
        // Read-only (main.rs): a terminal runs whatever it is given.
        if crate::read_only() {
            anyhow::bail!("read-only: no terminals");
        }
        Self::open(program, cwd, args, ctx)
    }

    /// Demo mode (demo.rs): a recorded session played into a terminal, in
    /// place of `claude attach`. Runs only the demo's own script.
    pub(super) fn replay(script: &Path, ctx: &egui::Context) -> Result<Self> {
        anyhow::ensure!(crate::demo::on(), "replays are for demo mode");
        let dir = script.parent().unwrap_or(Path::new("/"));
        Self::open(
            Path::new("/bin/sh"),
            dir,
            &[script.to_string_lossy().into_owned()],
            ctx,
        )
    }

    fn open(program: &Path, cwd: &Path, args: &[String], ctx: &egui::Context) -> Result<Self> {
        let size = (40, 120);
        let pair = native_pty_system().openpty(PtySize {
            rows: size.0,
            cols: size.1,
            pixel_width: 0,
            pixel_height: 0,
        })?;
        let mut command = CommandBuilder::new(program);
        command.args(args);
        command.cwd(cwd);
        command.env("TERM", "xterm-256color");
        command.env("COLORTERM", "truecolor");
        command.env("PATH", shell::user_path());
        for name in shell::SCRUB {
            command.env_remove(name);
        }
        let child = pair.slave.spawn_command(command)?;
        drop(pair.slave);

        let screen = Arc::new(Mutex::new(vt100::Parser::new_with_callbacks(
            size.0,
            size.1,
            5000,
            Title::default(),
        )));
        let mut reader = pair.master.try_clone_reader()?;
        let writer = pair.master.take_writer()?;
        let sink = screen.clone();
        let ctx = ctx.clone();
        // Reading blocks, so it gets its own thread; each chunk repaints.
        std::thread::Builder::new()
            .name("claude-pty".into())
            .spawn(move || {
                let mut buf = [0u8; 16 * 1024];
                while let Ok(n) = reader.read(&mut buf) {
                    if n == 0 {
                        break;
                    }
                    sink.lock()
                        .unwrap_or_else(PoisonError::into_inner)
                        .process(&buf[..n]);
                    ctx.request_repaint();
                }
                ctx.request_repaint();
            })?;
        Ok(Self {
            screen,
            writer,
            master: pair.master,
            child,
            size,
            selection: None,
            scroll_rest: 0.0,
            exited: false,
            spawned: Instant::now(),
            working_at: None,
        })
    }

    pub fn alive(&mut self) -> bool {
        if !self.exited {
            self.exited = !matches!(self.child.try_wait(), Ok(None));
        }
        !self.exited
    }

    /// Claude has drawn something and had two seconds to settle.
    pub(super) fn ready(&self) -> bool {
        self.spawned.elapsed() > Duration::from_secs(2)
            && !self
                .screen
                .lock()
                .unwrap_or_else(PoisonError::into_inner)
                .screen()
                .contents()
                .trim()
                .is_empty()
    }

    /// The program's process id, to find the Claude Code session it holds.
    pub fn pid(&self) -> Option<u32> {
        self.child.process_id()
    }

    /// What the screen shows now (screen.rs), once the program had three
    /// seconds to draw itself: the title and the last twelve non-empty rows.
    pub fn glance(&mut self) -> Option<super::screen::Seen> {
        use super::screen::Seen;
        if self.spawned.elapsed() < Duration::from_secs(3) {
            return None;
        }
        let seen = {
            let parser = self.screen.lock().unwrap_or_else(PoisonError::into_inner);
            let screen = parser.screen();
            let rows: Vec<String> = screen.rows(0, self.size.1).collect();
            let bottom: Vec<&str> = rows
                .iter()
                .rev()
                .map(|r| r.trim())
                .filter(|r| !r.is_empty())
                .take(12)
                .collect();
            super::screen::classify(&parser.callbacks().0, &bottom)
        };
        match seen {
            Some(Seen::Working) => self.working_at = Some(Instant::now()),
            Some(Seen::Prompt)
                if self
                    .working_at
                    .is_some_and(|at| at.elapsed() < Duration::from_secs(1)) =>
            {
                return Some(Seen::Working);
            }
            _ => {}
        }
        seen
    }

    /// Types a line into the program as if at the keyboard; the tty keeps
    /// it until the shell is ready to read.
    pub fn type_line(&mut self, line: &str) {
        self.write(format!("{}\r", plain(line)).as_bytes());
    }

    /// Text put into the program's prompt as one paste: inside bracketed
    /// paste markers when the program asked for them (Claude Code does), so
    /// a newline in it stays text and the Enter that follows is a key.
    pub(super) fn paste(&mut self, text: &str) {
        let bracketed = self
            .screen
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .screen()
            .bracketed_paste();
        self.write(&pasted(text, bracketed));
    }

    pub(super) fn write(&mut self, bytes: &[u8]) {
        let _ = self.writer.write_all(bytes);
        let _ = self.writer.flush();
    }

    fn resize(&mut self, rows: u16, cols: u16) {
        if self.size == (rows, cols) {
            return;
        }
        self.size = (rows, cols);
        self.screen
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .screen_mut()
            .set_size(rows, cols);
        let _ = self.master.resize(PtySize {
            rows,
            cols,
            pixel_width: 0,
            pixel_height: 0,
        });
    }
}

impl Drop for Terminal {
    fn drop(&mut self) {
        let _ = self.child.kill();
    }
}

/// Draws the terminal into the rest of `ui` and, while it has focus, sends
/// it the keyboard. Returns the link the user Cmd+clicked, if any.
pub fn show(ui: &mut Ui, terminal: &mut Terminal) -> Option<String> {
    let font = FontId::monospace(13.0);
    let (cell_w, cell_h) = ui.fonts_mut(|f| (f.glyph_width(&font, 'M'), f.row_height(&font)));
    let rect = ui.available_rect_before_wrap();
    let response = ui.allocate_rect(rect, Sense::click_and_drag());
    let id = response.id;
    if response.clicked() || response.drag_started() {
        response.request_focus();
    }
    // A pane that just got focus asks for its terminal to take the keys.
    let grab = egui::Id::new("terminal-grab");
    if ui
        .data(|d| d.get_temp::<Rect>(grab))
        .is_some_and(|area| area.contains_rect(rect))
    {
        response.request_focus();
        ui.data_mut(|d| d.remove::<Rect>(grab));
    }
    let focused = response.has_focus();
    if focused {
        // Tab, arrows and Escape belong to Claude, not to egui's focus moves.
        ui.memory_mut(|m| {
            m.set_focus_lock_filter(
                id,
                egui::EventFilter {
                    tab: true,
                    horizontal_arrows: true,
                    vertical_arrows: true,
                    escape: true,
                },
            );
        });
    }

    let pad = 8.0;
    let cols = ((rect.width() - 2.0 * pad) / cell_w).floor().max(20.0) as u16;
    let rows = ((rect.height() - 2.0 * pad) / cell_h).floor().max(5.0) as u16;
    terminal.resize(rows, cols);

    // Dragging selects between cell edges, snapped to the nearest one like a
    // text field, so the first and last letter aren't lost to a few pixels;
    // a plain click drops the selection.
    let origin = rect.min + vec2(pad, pad);
    let edge_at = |at: egui::Pos2| {
        let row = ((at.y - origin.y) / cell_h)
            .floor()
            .clamp(0.0, rows as f32 - 1.0) as u16;
        let col = ((at.x - origin.x) / cell_w).round().clamp(0.0, cols as f32) as u16;
        (row, col)
    };
    if response.drag_started()
        // drag_started fires after the pointer moved a few pixels: anchor where it went down.
        && let Some(at) = ui
            .input(|i| i.pointer.press_origin())
            .or(response.interact_pointer_pos())
    {
        let end = response.interact_pointer_pos().unwrap_or(at);
        terminal.selection = Some((edge_at(at), edge_at(end)));
    } else if response.dragged()
        && let (Some(at), Some((_, end))) =
            (response.interact_pointer_pos(), terminal.selection.as_mut())
    {
        *end = edge_at(at);
    } else if response.clicked() {
        terminal.selection = None;
    }
    if focused
        && let Some((a, b)) = terminal.selection
        && ui.input(|i| i.events.iter().any(|e| matches!(e, egui::Event::Copy)))
    {
        let text = selected_text(
            terminal
                .screen
                .lock()
                .unwrap_or_else(PoisonError::into_inner)
                .screen(),
            a,
            b,
        );
        ui.ctx().copy_text(text);
    }

    if focused {
        let input = input(
            ui,
            &terminal
                .screen
                .lock()
                .unwrap_or_else(PoisonError::into_inner),
        );
        if !input.is_empty() {
            terminal.selection = None;
            terminal
                .screen
                .lock()
                .unwrap_or_else(PoisonError::into_inner)
                .screen_mut()
                .set_scrollback(0);
            terminal.write(&input);
        }
    }
    if response.hovered() {
        let (lines, rest) = scroll_lines(
            terminal.scroll_rest,
            ui.input(|i| i.smooth_scroll_delta.y),
            cell_h,
        );
        terminal.scroll_rest = rest;
        if lines != 0 {
            terminal.selection = None;
            let mut parser = terminal
                .screen
                .lock()
                .unwrap_or_else(PoisonError::into_inner);
            let screen = parser.screen();
            // A full-screen program (k9s, less) has no scrollback: it gets
            // the wheel itself when it asked for the mouse.
            let wheel = screen.alternate_screen()
                && screen.mouse_protocol_mode() != vt100::MouseProtocolMode::None;
            if wheel {
                let sgr = screen.mouse_protocol_encoding() == vt100::MouseProtocolEncoding::Sgr;
                drop(parser);
                let bytes = wheel_bytes(lines, sgr);
                terminal.write(&bytes);
            } else {
                let back = (screen.scrollback() as i64 + lines).max(0) as usize;
                parser.screen_mut().set_scrollback(back);
            }
        }
    }

    let painter = ui.painter_at(rect);
    painter.rect_filled(rect, 0.0, theme::p().bg);
    let parser = terminal
        .screen
        .lock()
        .unwrap_or_else(PoisonError::into_inner);
    let screen = parser.screen();
    if let Some((a, b)) = terminal.selection {
        let ((top, left), (bottom, right)) = ordered(a, b);
        for row in top..=bottom {
            let from = if row == top { left } else { 0 };
            let to = if row == bottom { right } else { cols };
            painter.rect_filled(
                Rect::from_min_size(
                    pos2(
                        origin.x + from as f32 * cell_w,
                        origin.y + row as f32 * cell_h,
                    ),
                    vec2((to - from) as f32 * cell_w, cell_h),
                ),
                0.0,
                theme::p().signal.gamma_multiply(0.35),
            );
        }
    }

    // Cmd over a link shows a hand; Cmd+click opens it (in Orbit when it is
    // a ticket or a PR, see Action::Link).
    let mut link = None;
    if ui.input(|i| i.modifiers.command)
        && let Some(at) = response.hover_pos()
    {
        let (row, col) = (
            ((at.y - origin.y) / cell_h).floor(),
            ((at.x - origin.x) / cell_w).floor(),
        );
        if row >= 0.0 && col >= 0.0 && (row as u16) < rows {
            let (line, col) = logical_line(screen, row as u16, col as usize, cols);
            if let Some(url) = url_at(&line, col) {
                ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
                if response.clicked() {
                    link = Some(url);
                }
            }
        }
    }
    for row in 0..rows {
        let y = origin.y + row as f32 * cell_h;
        // Runs of plain-width text in one style are drawn together; anything
        // else is drawn at its own cell so columns stay aligned.
        let mut run = String::new();
        let mut run_col = 0;
        let mut run_style: Option<(Color32, bool)> = None;
        let flush = |run: &mut String, col: u16, style: Option<(Color32, bool)>| {
            if let Some((color, _)) = style
                && !run.trim().is_empty()
            {
                painter.text(
                    pos2(origin.x + col as f32 * cell_w, y),
                    egui::Align2::LEFT_TOP,
                    run.as_str(),
                    font.clone(),
                    color,
                );
            }
            run.clear();
        };
        for col in 0..cols {
            let Some(cell) = screen.cell(row, col) else {
                continue;
            };
            if cell.is_wide_continuation() {
                continue;
            }
            let (mut fg, mut bg) = (color(cell.fgcolor(), true), color(cell.bgcolor(), false));
            if cell.inverse() {
                std::mem::swap(&mut fg, &mut bg);
            }
            if cell.dim() {
                fg = fg.lerp_to_gamma(bg, 0.4);
            }
            fg = theme::readable(fg, bg);
            let width = if cell.is_wide() { 2.0 } else { 1.0 };
            if bg != theme::p().bg {
                painter.rect_filled(
                    Rect::from_min_size(
                        pos2(origin.x + col as f32 * cell_w, y),
                        vec2(cell_w * width, cell_h),
                    ),
                    0.0,
                    bg,
                );
            }
            let text = if cell.has_contents() {
                cell.contents()
            } else {
                " "
            };
            let plain = text.len() == 1 && !cell.is_wide();
            let style = Some((fg, cell.bold()));
            if !plain || style != run_style {
                flush(&mut run, run_col, run_style);
                run_col = col;
                run_style = style;
            }
            if plain {
                run.push_str(text);
            } else {
                painter.text(
                    pos2(origin.x + col as f32 * cell_w, y),
                    egui::Align2::LEFT_TOP,
                    text,
                    font.clone(),
                    fg,
                );
                run_col = col + width as u16;
            }
        }
        flush(&mut run, run_col, run_style);
    }
    if focused && !screen.hide_cursor() && screen.scrollback() == 0 {
        let (row, col) = screen.cursor_position();
        painter.rect_filled(
            Rect::from_min_size(
                pos2(
                    origin.x + col as f32 * cell_w,
                    origin.y + row as f32 * cell_h,
                ),
                vec2(cell_w, cell_h),
            ),
            0.0,
            theme::p().signal.gamma_multiply(0.7),
        );
    }
    if !focused {
        // On its own background: the hint sits over the first rows.
        let galley = painter.layout_no_wrap(
            "click to type".into(),
            FontId::monospace(11.0),
            theme::p().text_3,
        );
        let at = rect.right_top() + vec2(-pad - galley.size().x, pad);
        painter.rect_filled(
            Rect::from_min_size(at, galley.size()).expand(4.0),
            0.0,
            theme::p().bg,
        );
        painter.galley(at, galley, theme::p().text_3);
    }
    link
}

/// Two cells in reading order.
fn ordered(a: (u16, u16), b: (u16, u16)) -> ((u16, u16), (u16, u16)) {
    if a <= b { (a, b) } else { (b, a) }
}

/// The text of the cells between the edges `a` and `b` (either order),
/// rows the terminal wrapped joined, trailing blanks dropped.
fn selected_text(screen: &vt100::Screen, a: (u16, u16), b: (u16, u16)) -> String {
    let ((top, left), (bottom, right)) = ordered(a, b);
    screen.contents_between(top, left, bottom, right)
}

/// The cells of the line `row` belongs to, rows the terminal wrapped joined,
/// and where column `col` of `row` lands in them.
// ponytail: only terminal wraps join; a URL Claude itself broke over two rows
// opens its first part.
fn logical_line(screen: &vt100::Screen, row: u16, col: usize, cols: u16) -> (Vec<String>, usize) {
    let mut first = row;
    while first > 0 && screen.row_wrapped(first - 1) {
        first -= 1;
    }
    let mut last = row;
    while screen.row_wrapped(last) && last + 1 < screen.size().0 {
        last += 1;
    }
    let cells = (first..=last)
        .flat_map(|r| (0..cols).map(move |c| (r, c)))
        .map(|(r, c)| {
            screen
                .cell(r, c)
                .map(|c| c.contents().to_owned())
                .unwrap_or_default()
        })
        .collect();
    (cells, (row - first) as usize * cols as usize + col)
}

/// The http(s) URL covering cell `col`, trailing punctuation dropped; or
/// an `orbit://session/<key>` link, which Orbit opens itself.
fn url_at(cells: &[String], col: usize) -> Option<String> {
    let blank = |c: &String| c.is_empty() || c.chars().all(char::is_whitespace);
    if cells.get(col).is_none_or(blank) {
        return None;
    }
    let start = cells[..col].iter().rposition(blank).map_or(0, |i| i + 1);
    let end = cells[col..]
        .iter()
        .position(blank)
        .map_or(cells.len(), |i| col + i);
    let word: String = cells[start..end].concat();
    let at = word
        .find("https://")
        .or_else(|| word.find("http://"))
        .or_else(|| word.find("orbit://"))?;
    let url = word[at..].trim_end_matches(['.', ',', ';', ':', ')', ']', '>', '\'', '"', '`']);
    (url.len() > "https://".len()).then(|| url.to_owned())
}

/// Whole lines to scroll (up is positive) from this frame's points, and the
/// part of a line left over for the next frame.
fn scroll_lines(rest: f32, delta: f32, cell_h: f32) -> (i64, f32) {
    let total = rest + delta;
    let lines = (total / cell_h).trunc();
    (lines as i64, total - lines * cell_h)
}

/// The mouse wheel as a terminal reports it at the top-left cell: one
/// event per line, up for positive.
fn wheel_bytes(lines: i64, sgr: bool) -> Vec<u8> {
    let button = if lines > 0 { 64 } else { 65 };
    let one = if sgr {
        format!("\x1b[<{button};1;1M").into_bytes()
    } else {
        vec![0x1b, b'[', b'M', 32 + button as u8, 33, 33]
    };
    one.repeat(lines.unsigned_abs() as usize)
}

/// A process's working folder, from macOS's lsof (by its full path: an app
/// started from Finder has no PATH).
pub(super) fn cwd_of(pid: u32) -> Option<std::path::PathBuf> {
    let out = std::process::Command::new("/usr/sbin/lsof")
        .args(["-a", "-p", &pid.to_string(), "-d", "cwd", "-Fn"])
        .output()
        .ok()?;
    lsof_cwd(&String::from_utf8_lossy(&out.stdout))
}

/// lsof's `-Fn` answer: the line after `n` is the path.
fn lsof_cwd(text: &str) -> Option<std::path::PathBuf> {
    text.lines()
        .find_map(|l| l.strip_prefix('n'))
        .filter(|p| p.starts_with('/'))
        .map(std::path::PathBuf::from)
}

/// Keyboard events as the bytes a terminal would send.
fn input(ui: &Ui, parser: &vt100::Parser<Title>) -> Vec<u8> {
    let screen = parser.screen();
    let cursor = |normal: &'static [u8], app: &'static [u8]| {
        if screen.application_cursor() {
            app
        } else {
            normal
        }
    };
    let mut out = Vec::new();
    ui.input(|i| {
        for event in &i.events {
            match event {
                egui::Event::Text(text) => out.extend_from_slice(text.as_bytes()),
                egui::Event::Paste(text) => {
                    if screen.bracketed_paste() {
                        out.extend_from_slice(b"\x1b[200~");
                        out.extend_from_slice(text.as_bytes());
                        out.extend_from_slice(b"\x1b[201~");
                    } else {
                        out.extend_from_slice(text.as_bytes());
                    }
                }
                egui::Event::Key {
                    key,
                    pressed: true,
                    modifiers,
                    ..
                } => {
                    use egui::Key;
                    if modifiers.ctrl
                        && let Some(letter) = key
                            .name()
                            .chars()
                            .next()
                            .filter(|c| c.is_ascii_alphabetic())
                        && key.name().len() == 1
                    {
                        out.push(letter.to_ascii_lowercase() as u8 - b'a' + 1);
                        continue;
                    }
                    let bytes: &[u8] = match key {
                        // Meta-Enter is Claude Code's newline in the prompt.
                        Key::Enter if modifiers.shift || modifiers.alt => b"\x1b\r",
                        Key::Enter => b"\r",
                        Key::Backspace if modifiers.alt => b"\x1b\x7f",
                        Key::Backspace => b"\x7f",
                        Key::Tab if modifiers.shift => b"\x1b[Z",
                        Key::Tab => b"\t",
                        Key::Escape => b"\x1b",
                        Key::ArrowUp => cursor(b"\x1b[A", b"\x1bOA"),
                        Key::ArrowDown => cursor(b"\x1b[B", b"\x1bOB"),
                        Key::ArrowRight => cursor(b"\x1b[C", b"\x1bOC"),
                        Key::ArrowLeft => cursor(b"\x1b[D", b"\x1bOD"),
                        Key::Home => b"\x1b[H",
                        Key::End => b"\x1b[F",
                        Key::Delete => b"\x1b[3~",
                        Key::PageUp => b"\x1b[5~",
                        Key::PageDown => b"\x1b[6~",
                        _ => b"",
                    };
                    out.extend_from_slice(bytes);
                }
                _ => {}
            }
        }
    });
    out
}

/// xterm's 256 colours, with the default foreground and background.
fn color(color: vt100::Color, foreground: bool) -> Color32 {
    match color {
        vt100::Color::Default if foreground => theme::p().text,
        vt100::Color::Default => theme::p().bg,
        vt100::Color::Rgb(r, g, b) => Color32::from_rgb(r, g, b),
        vt100::Color::Idx(i) if i < 16 => theme::ANSI[i as usize],
        vt100::Color::Idx(i) if i < 232 => {
            let i = i - 16;
            let level = |v: u8| if v == 0 { 0 } else { 55 + v * 40 };
            Color32::from_rgb(level(i / 36), level((i / 6) % 6), level(i % 6))
        }
        vt100::Color::Idx(i) => Color32::from_gray(8 + (i - 232) * 10),
    }
}

/// The bytes of `text` as a paste: wrapped in bracketed paste markers
/// when the program turned that mode on.
fn pasted(text: &str, bracketed: bool) -> Vec<u8> {
    let text = plain(text);
    if bracketed {
        format!("\x1b[200~{text}\x1b[201~").into_bytes()
    } else {
        text.into_bytes()
    }
}

/// Text with no control characters but newline and tab: an escape inside
/// it could end the bracketed paste (`ESC[201~`) and turn the rest into
/// keys, such as Shift+Tab that switches a session's permission mode.
fn plain(text: &str) -> String {
    text.chars()
        .filter(|c| !c.is_control() || matches!(c, '\n' | '\t'))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_paste_cannot_end_itself_or_press_keys() {
        let evil = "fix it\x1b[201~\x1b[Z\x1b[Z\r\x7f\nnext\tline";
        let bytes = String::from_utf8(pasted(evil, true)).unwrap();
        assert_eq!(bytes, "\x1b[200~fix it[201~[Z[Z\nnext\tline\x1b[201~");
        assert_eq!(
            bytes.matches('\x1b').count(),
            2,
            "only the paste's own markers"
        );
        assert_eq!(plain("a\rb\x07c"), "abc");
    }

    #[test]
    fn a_poisoned_screen_still_draws() {
        let screen = std::sync::Arc::new(std::sync::Mutex::new(vt100::Parser::new(2, 10, 0)));
        let reader = screen.clone();
        let _ = std::thread::spawn(move || {
            let _guard = reader.lock().unwrap();
            panic!("vt100 choked");
        })
        .join();
        assert!(screen.is_poisoned());
        let parser = screen.lock().unwrap_or_else(PoisonError::into_inner);
        assert_eq!(parser.screen().size(), (2, 10));
    }

    #[test]
    fn a_line_is_pasted_inside_the_markers_the_program_asked_for() {
        assert_eq!(
            pasted("/build DEV-1", true),
            b"\x1b[200~/build DEV-1\x1b[201~".to_vec()
        );
        assert_eq!(pasted("/build DEV-1", false), b"/build DEV-1".to_vec());
    }

    #[test]
    fn a_split_shell_finds_where_the_shell_is() {
        assert_eq!(
            lsof_cwd("p123\nfcwd\nn/Users/k/Development/app\n"),
            Some("/Users/k/Development/app".into())
        );
        assert_eq!(lsof_cwd(""), None);
        // This test's own process, for real.
        assert_eq!(cwd_of(std::process::id()), std::env::current_dir().ok());
    }

    #[test]
    fn a_trackpads_small_steps_add_up_to_lines() {
        // 4 points a frame against 17-point rows: nothing for four frames,
        // then a line, never lost to rounding.
        let mut rest = 0.0;
        let mut lines = 0;
        for _ in 0..5 {
            let (n, r) = scroll_lines(rest, 4.0, 17.0);
            lines += n;
            rest = r;
        }
        assert_eq!(lines, 1);
        assert!((rest - 3.0).abs() < 1e-4);
        let (down, _) = scroll_lines(0.0, -40.0, 17.0);
        assert_eq!(down, -2);
        assert_eq!(wheel_bytes(2, true), b"\x1b[<64;1;1M\x1b[<64;1;1M".to_vec());
        assert_eq!(wheel_bytes(-1, false), vec![0x1b, b'[', b'M', 97, 33, 33]);
    }

    #[test]
    fn a_link_is_found_from_any_of_its_cells() {
        let cells: Vec<String> = "see (https://github.com/o/r/pull/7). ok"
            .chars()
            .map(String::from)
            .collect();
        let url = Some("https://github.com/o/r/pull/7".to_owned());
        let orbit: Vec<String> = "open orbit://session/free:17. now"
            .chars()
            .map(String::from)
            .collect();
        assert_eq!(
            super::url_at(&orbit, 8).as_deref(),
            Some("orbit://session/free:17"),
            "a session link, its full stop dropped"
        );
        assert_eq!(url_at(&cells, 5), url);
        assert_eq!(url_at(&cells, 33), url);
        assert_eq!(url_at(&cells, 1), None);
        assert_eq!(url_at(&cells, 3), None);

        // Wrapped by the terminal over two rows: a click on either row finds it.
        let mut parser = vt100::Parser::new(5, 20, 0);
        parser.process(b"go https://linear.app/a/issue/ENG-1 now");
        for (row, col) in [(0, 5), (1, 3)] {
            let (line, col) = logical_line(parser.screen(), row, col, 20);
            assert_eq!(
                url_at(&line, col).as_deref(),
                Some("https://linear.app/a/issue/ENG-1")
            );
        }
    }

    #[test]
    fn a_drag_copies_the_cells_between_its_ends_in_either_direction() {
        let mut parser = vt100::Parser::new(5, 20, 0);
        parser.process(b"hello world\r\nsecond line");
        let screen = parser.screen();
        assert_eq!(selected_text(screen, (0, 6), (0, 11)), "world");
        assert_eq!(selected_text(screen, (1, 6), (0, 6)), "world\nsecond");
    }

    #[test]
    fn xterm_palette_corners() {
        assert_eq!(
            color(vt100::Color::Idx(16), true),
            Color32::from_rgb(0, 0, 0)
        );
        assert_eq!(
            color(vt100::Color::Idx(231), true),
            Color32::from_rgb(255, 255, 255)
        );
        assert_eq!(color(vt100::Color::Idx(255), true), Color32::from_gray(238));
        assert_eq!(color(vt100::Color::Default, false), theme::p().bg);
    }
}
