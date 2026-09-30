//! In-process UTF-8 editor. TUI callers drive the existing terminal event
//! stream.
use anyhow::{Context, bail};
use ratatui::{
    Frame, Terminal,
    backend::CrosstermBackend,
    layout::Rect,
    style::{Modifier, Style},
    text::{Line, Span},
    widgets::Paragraph,
};
use std::{
    fmt, fs,
    io::{self, IsTerminal, Write},
    ops::Range,
    path::{Path, PathBuf},
};
use terminput::{Event, KeyCode, KeyEventKind, KeyModifiers};
use unicode_segmentation::UnicodeSegmentation;
use unicode_width::UnicodeWidthStr;

#[derive(Clone)]
struct Snapshot {
    text: String,
    cursor: usize,
}

/// Document contents are deliberately excluded from diagnostics.
pub struct FileEditor {
    path: PathBuf,
    saved: String,
    state: Snapshot,
    anchor: Option<usize>,
    undo: Vec<Snapshot>,
    redo: Vec<Snapshot>,
    clipboard: String,
    top: usize,
    left: usize,
    height: usize,
    readonly: bool,
    confirm: bool,
    searching: bool,
    search: String,
    status: String,
    newline: &'static str,
}
impl fmt::Debug for FileEditor {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("FileEditor")
            .field("readonly", &self.readonly)
            .finish_non_exhaustive()
    }
}
#[derive(Debug, PartialEq, Eq)]
pub enum EditorAction {
    Continue,
    Saved,
    Close,
}
impl FileEditor {
    pub fn open(path: &Path, readonly: bool) -> anyhow::Result<Self> {
        let path = path.canonicalize().context("Cannot open file")?;
        let bytes = fs::read(&path).context("Cannot read file")?;
        let text = match String::from_utf8(bytes) {
            Ok(text) => text,
            Err(error) if readonly => {
                use std::fmt::Write as _;
                let mut text = String::new();
                for (offset, row) in error.as_bytes().chunks(16).enumerate() {
                    write!(text, "{:08x}  ", offset * 16)?;
                    for byte in row {
                        write!(text, "{byte:02x} ")?;
                    }
                    text.push('\n');
                }
                text
            }
            Err(_) => bail!(
                "File is not UTF-8 text; use the built-in viewer for binary data"
            ),
        };
        let newline = if text.contains("\r\n") { "\r\n" } else { "\n" };
        Ok(Self {
            path,
            saved: text.clone(),
            state: Snapshot { text, cursor: 0 },
            anchor: None,
            undo: Vec::new(),
            redo: Vec::new(),
            clipboard: String::new(),
            top: 0,
            left: 0,
            height: 20,
            readonly,
            confirm: false,
            searching: false,
            search: String::new(),
            status: String::new(),
            newline,
        })
    }
    /// Source locations are one-based; stale locations are safely clamped.
    pub fn goto(&mut self, line: usize, column: usize) {
        let start = self
            .state
            .text
            .split_inclusive('\n')
            .take(line.saturating_sub(1))
            .map(str::len)
            .sum::<usize>()
            .min(self.state.text.len());
        let end = self.state.text[start..]
            .find('\n')
            .map_or(self.state.text.len(), |n| start + n);
        self.state.cursor = start
            + self.state.text[start..end]
                .char_indices()
                .nth(column.saturating_sub(1))
                .map_or(end - start, |(i, _)| i);
    }
    fn dirty(&self) -> bool {
        self.state.text != self.saved
    }
    fn selection(&self) -> Range<usize> {
        let anchor = self.anchor.unwrap_or(self.state.cursor);
        anchor.min(self.state.cursor)..anchor.max(self.state.cursor)
    }
    fn previous(&self) -> usize {
        self.state.text[..self.state.cursor]
            .grapheme_indices(true)
            .next_back()
            .map_or(0, |(i, _)| i)
    }
    fn next(&self) -> usize {
        self.state.cursor
            + self.state.text[self.state.cursor..]
                .graphemes(true)
                .next()
                .map_or(0, str::len)
    }
    fn line_start(&self) -> usize {
        self.state.text[..self.state.cursor]
            .rfind('\n')
            .map_or(0, |i| i + 1)
    }
    fn line_end(&self) -> usize {
        let end = self.state.text[self.state.cursor..]
            .find('\n')
            .map_or(self.state.text.len(), |i| self.state.cursor + i);
        if end > 0 && self.state.text.as_bytes()[end - 1] == b'\r' {
            end - 1
        } else {
            end
        }
    }
    fn move_to(&mut self, cursor: usize, selecting: bool) {
        if selecting {
            self.anchor.get_or_insert(self.state.cursor);
        } else {
            self.anchor = None;
        }
        self.state.cursor = cursor;
    }
    fn vertical(&mut self, down: bool, count: usize, selecting: bool) {
        let column = self.state.text[self.line_start()..self.state.cursor]
            .graphemes(true)
            .count();
        let row = self.state.text[..self.state.cursor]
            .bytes()
            .filter(|b| *b == b'\n')
            .count();
        let rows = self.state.text.split('\n').collect::<Vec<_>>();
        let new_row = if down {
            row.saturating_add(count).min(rows.len() - 1)
        } else {
            row.saturating_sub(count)
        };
        let line = rows[new_row].trim_end_matches('\r');
        let column = line
            .grapheme_indices(true)
            .nth(column)
            .map_or(line.len(), |(i, _)| i);
        let start = rows[..new_row]
            .iter()
            .map(|line| line.len() + 1)
            .sum::<usize>();
        self.move_to(start + column, selecting);
    }
    fn replace(&mut self, range: Range<usize>, value: &str) {
        if self.readonly {
            return;
        }
        self.undo.push(self.state.clone());
        while self.undo.len() > 100
            || (self.undo.len() > 1
                && self.undo.iter().map(|s| s.text.len()).sum::<usize>()
                    > 32 * 1024 * 1024)
        {
            self.undo.remove(0);
        }
        self.redo.clear();
        self.state.text.replace_range(range.clone(), value);
        self.state.cursor = range.start + value.len();
        self.anchor = None;
        self.status.clear();
    }
    fn save(&mut self) -> anyhow::Result<()> {
        if self.readonly {
            bail!("Viewer is read-only");
        }
        if !self.dirty() {
            return Ok(());
        }
        if fs::read(&self.path)? != self.saved.as_bytes() {
            bail!(
                "File changed on disk. Close and reopen to avoid overwriting another edit"
            );
        }
        let permissions = fs::metadata(&self.path)?.permissions();
        if permissions.readonly() {
            bail!("File is read-only");
        }
        let parent =
            self.path.parent().context("File has no parent directory")?;
        let mut temporary = tempfile::NamedTempFile::new_in(parent)?;
        temporary.write_all(self.state.text.as_bytes())?;
        temporary.flush()?;
        temporary.as_file().sync_all()?;
        temporary.as_file().set_permissions(permissions)?;
        if fs::read(&self.path)? != self.saved.as_bytes() {
            bail!("File changed on disk while saving. Reopen before saving");
        }
        temporary.persist(&self.path).map_err(|error| error.error)?;
        self.saved.clone_from(&self.state.text);
        Ok(())
    }
    fn save_action(&mut self, close: bool) -> EditorAction {
        match self.save() {
            Ok(()) => {
                self.confirm = false;
                self.status = "Saved".into();
                if close {
                    EditorAction::Close
                } else {
                    EditorAction::Saved
                }
            }
            Err(error) => {
                self.status = format!("Save failed: {error}");
                self.confirm = false;
                EditorAction::Continue
            }
        }
    }
    fn find_next(&mut self) {
        if self.search.is_empty() {
            return;
        }
        let start = self.next();
        let found = self.state.text[start..]
            .find(&self.search)
            .map(|i| start + i)
            .or_else(|| self.state.text[..start].find(&self.search));
        if let Some(index) = found {
            self.state.cursor = index;
            self.anchor = None;
            self.status.clear();
        } else {
            self.status = "No match".into();
        }
    }
    pub fn handle_event(&mut self, event: Event) -> EditorAction {
        let Event::Key(key) = event else {
            if let Event::Paste(text) = event {
                if self.searching {
                    self.search.push_str(&text);
                } else if !self.confirm {
                    self.replace(self.selection(), &text);
                }
            }
            return EditorAction::Continue;
        };
        if key.kind == KeyEventKind::Release {
            return EditorAction::Continue;
        }
        let ctrl = key.modifiers.contains(KeyModifiers::CTRL);
        let shift = key.modifiers.contains(KeyModifiers::SHIFT);
        if self.confirm {
            return match key.code {
                KeyCode::Char('y' | 'Y') => EditorAction::Close,
                KeyCode::Char('s' | 'S') => self.save_action(true),
                KeyCode::Esc | KeyCode::Char('n' | 'N') => {
                    self.confirm = false;
                    EditorAction::Continue
                }
                _ => EditorAction::Continue,
            };
        }
        if self.searching {
            match key.code {
                KeyCode::Esc => self.searching = false,
                KeyCode::Enter => {
                    self.searching = false;
                    self.find_next();
                }
                KeyCode::Backspace => {
                    self.search.pop();
                }
                KeyCode::Char(c) if !ctrl => self.search.push(c),
                _ => {}
            }
            return EditorAction::Continue;
        }
        match key.code {
            KeyCode::Esc | KeyCode::Char('q')
                if key.code == KeyCode::Esc || ctrl || self.readonly =>
            {
                if self.dirty() {
                    self.confirm = true;
                } else {
                    return EditorAction::Close;
                }
            }
            KeyCode::F(2) if !self.readonly => return self.save_action(true),
            KeyCode::Char('s') if ctrl && !self.readonly => {
                return self.save_action(false);
            }
            KeyCode::Char('a') if ctrl => {
                self.anchor = Some(0);
                self.state.cursor = self.state.text.len();
            }
            KeyCode::Char('z') if ctrl && !self.readonly => {
                if let Some(old) = self.undo.pop() {
                    self.redo.push(std::mem::replace(&mut self.state, old));
                    self.anchor = None;
                }
            }
            KeyCode::Char('y') if ctrl && !self.readonly => {
                if let Some(new) = self.redo.pop() {
                    self.undo.push(std::mem::replace(&mut self.state, new));
                    self.anchor = None;
                }
            }
            KeyCode::Char('c' | 'x') if ctrl => {
                self.clipboard = self.state.text[self.selection()].to_owned();
                if key.code == KeyCode::Char('x') {
                    self.replace(self.selection(), "");
                }
            }
            KeyCode::Char('v') if ctrl => {
                let value = self.clipboard.clone();
                self.replace(self.selection(), &value);
            }
            KeyCode::Char('f') if ctrl => {
                self.search.clear();
                self.searching = true;
            }
            KeyCode::F(3) => self.find_next(),
            KeyCode::Left => self.move_to(self.previous(), shift),
            KeyCode::Right => self.move_to(self.next(), shift),
            KeyCode::Home => {
                self.move_to(if ctrl { 0 } else { self.line_start() }, shift)
            }
            KeyCode::End => self.move_to(
                if ctrl {
                    self.state.text.len()
                } else {
                    self.line_end()
                },
                shift,
            ),
            KeyCode::Up => self.vertical(false, 1, shift),
            KeyCode::Down => self.vertical(true, 1, shift),
            KeyCode::PageUp => self.vertical(false, self.height, shift),
            KeyCode::PageDown => self.vertical(true, self.height, shift),
            KeyCode::Backspace => {
                let range = self.selection();
                self.replace(
                    if range.is_empty() {
                        self.previous()..self.state.cursor
                    } else {
                        range
                    },
                    "",
                );
            }
            KeyCode::Delete => {
                let range = self.selection();
                self.replace(
                    if range.is_empty() {
                        self.state.cursor..self.next()
                    } else {
                        range
                    },
                    "",
                );
            }
            KeyCode::Enter => self.replace(self.selection(), self.newline),
            KeyCode::Tab => self.replace(self.selection(), "  "),
            KeyCode::Char(c)
                if (key.modifiers - KeyModifiers::SHIFT).is_empty() =>
            {
                self.replace(self.selection(), &c.to_string())
            }
            _ => {}
        }
        EditorAction::Continue
    }
    pub fn draw(&mut self, frame: &mut Frame<'_>) {
        let area = frame.area();
        if area.height < 3 || area.width < 4 {
            return;
        }
        self.height = usize::from(area.height - 2);
        let row = self.state.text[..self.state.cursor]
            .bytes()
            .filter(|b| *b == b'\n')
            .count();
        let column =
            display(&self.state.text[self.line_start()..self.state.cursor])
                .width();
        let width = usize::from(area.width);
        self.top = self
            .top
            .min(row)
            .max(row.saturating_add(1).saturating_sub(self.height));
        self.left = self
            .left
            .min(column)
            .max(column.saturating_add(1).saturating_sub(width));
        let mode = if self.readonly {
            "Built-in viewer"
        } else {
            "Built-in editor"
        };
        let title = format!(
            "{mode} {}{}",
            display(&self.path.display().to_string()),
            if self.dirty() { " *" } else { "" }
        );
        let reversed = Style::new().add_modifier(Modifier::REVERSED);
        frame.render_widget(
            Paragraph::new(title).style(reversed),
            Rect::new(area.x, area.y, area.width, 1),
        );
        let selected = self.selection();
        let mut offset = 0;
        let mut lines = Vec::new();
        for (index, line) in self.state.text.split('\n').enumerate() {
            if index >= self.top + self.height {
                break;
            }
            if index >= self.top {
                let mut spans = Vec::new();
                let mut cell = 0;
                for (byte, grapheme) in line.grapheme_indices(true) {
                    let visible = display(grapheme);
                    let n = visible.width();
                    if cell >= self.left && cell + n <= self.left + width {
                        let style = if selected.contains(&(offset + byte)) {
                            reversed
                        } else {
                            Style::new()
                        };
                        spans.push(Span::styled(visible, style));
                    } else if cell < self.left && cell + n > self.left {
                        spans.push(Span::raw(" ".repeat(cell + n - self.left)));
                    }
                    cell += n;
                }
                lines.push(Line::from(spans));
            }
            offset += line.len() + 1;
        }
        frame.render_widget(
            Paragraph::new(lines),
            Rect::new(area.x, area.y + 1, area.width, area.height - 2),
        );
        let footer = if self.confirm {
            "Unsaved changes: Y discard / N keep editing / S save and close"
                .into()
        } else if self.searching {
            format!("Find: {}", display(&self.search))
        } else if !self.status.is_empty() {
            self.status.clone()
        } else if self.readonly {
            "Esc/Q close | Arrows/PgUp/PgDn | Ctrl+F find | F3 next".into()
        } else {
            "Ctrl+S save | F2 save/close | Esc close | Ctrl+Z/Y undo/redo | Ctrl+F find".into()
        };
        frame.render_widget(
            Paragraph::new(footer).style(reversed),
            Rect::new(area.x, area.bottom() - 1, area.width, 1),
        );
        if !self.readonly && !self.confirm && !self.searching {
            frame.set_cursor_position((
                area.x + (column - self.left) as u16,
                area.y + 1 + (row - self.top) as u16,
            ));
        }
    }
}
fn display(text: &str) -> String {
    text.chars()
        .map(|c| match c {
            '\t' => "    ".into(),
            '\r' | '\u{feff}' => String::new(),
            c if c.is_control() => "�".into(),
            c => c.to_string(),
        })
        .collect()
}
/// CLI frontend; the TUI uses its existing event stream instead.
pub fn edit_file(path: &Path) -> anyhow::Result<()> {
    use crossterm::{
        event::{DisableBracketedPaste, EnableBracketedPaste},
        execute,
        terminal::{self, EnterAlternateScreen, LeaveAlternateScreen},
    };
    if !io::stdin().is_terminal() || !io::stdout().is_terminal() {
        bail!("Built-in editing requires an interactive terminal");
    }
    let mut editor = FileEditor::open(path, false)?;
    struct Restore;
    impl Drop for Restore {
        fn drop(&mut self) {
            let _ = terminal::disable_raw_mode();
            let _ = execute!(
                io::stdout(),
                DisableBracketedPaste,
                LeaveAlternateScreen,
                crossterm::cursor::Show
            );
        }
    }
    terminal::enable_raw_mode()?;
    let _restore = Restore;
    execute!(io::stdout(), EnterAlternateScreen, EnableBracketedPaste)?;
    let mut terminal = Terminal::new(CrosstermBackend::new(io::stdout()))?;
    loop {
        terminal.draw(|frame| editor.draw(frame))?;
        if let Ok(event) =
            terminput_crossterm::to_terminput(crossterm::event::read()?)
            && editor.handle_event(event) == EditorAction::Close
        {
            break;
        }
    }
    Ok(())
}
/// Atomically export to a new file, refusing to overwrite an existing file.
pub fn save_new(path: &Path, bytes: &[u8]) -> anyhow::Result<()> {
    let parent = path
        .parent()
        .filter(|p| !p.as_os_str().is_empty())
        .unwrap_or_else(|| Path::new("."));
    let mut temporary = tempfile::NamedTempFile::new_in(parent)?;
    temporary.write_all(bytes)?;
    temporary.flush()?;
    temporary.as_file().sync_all()?;
    temporary
        .persist_noclobber(path)
        .map_err(|error| error.error)?;
    Ok(())
}
#[cfg(test)]
mod tests {
    use super::*;
    use terminput::{KeyEvent, KeyEventState};
    fn key(
        editor: &mut FileEditor,
        code: KeyCode,
        modifiers: KeyModifiers,
    ) -> EditorAction {
        editor.handle_event(Event::Key(KeyEvent {
            code,
            modifiers,
            kind: KeyEventKind::Press,
            state: KeyEventState::empty(),
        }))
    }
    fn fixture(text: &[u8]) -> (tempfile::TempDir, PathBuf) {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("中文 空格.yml");
        fs::write(&path, text).unwrap();
        (dir, path)
    }
    #[test]
    fn unicode_paste_undo_save() {
        let (_dir, path) = fixture(b"name: old\r\n");
        let mut editor = FileEditor::open(&path, false).unwrap();
        key(&mut editor, KeyCode::Char('a'), KeyModifiers::CTRL);
        editor.handle_event(Event::Paste("name: 中文👩‍💻\r\n".into()));
        key(&mut editor, KeyCode::Char('z'), KeyModifiers::CTRL);
        assert_eq!(editor.state.text, "name: old\r\n");
        key(&mut editor, KeyCode::Char('y'), KeyModifiers::CTRL);
        assert_eq!(
            key(&mut editor, KeyCode::F(2), KeyModifiers::empty()),
            EditorAction::Close
        );
        assert_eq!(fs::read_to_string(path).unwrap(), "name: 中文👩‍💻\r\n");
    }
    #[test]
    fn cancel_does_not_write_and_conflict_keeps_buffer() {
        let (_dir, path) = fixture(b"original");
        let mut editor = FileEditor::open(&path, false).unwrap();
        editor.handle_event(Event::Paste("changed".into()));
        assert_eq!(
            key(&mut editor, KeyCode::Esc, KeyModifiers::empty()),
            EditorAction::Continue
        );
        assert_eq!(
            key(&mut editor, KeyCode::Char('y'), KeyModifiers::empty()),
            EditorAction::Close
        );
        assert_eq!(fs::read_to_string(&path).unwrap(), "original");
        fs::write(&path, "another writer").unwrap();
        assert!(editor.save().is_err());
        assert!(editor.state.text.contains("changed"));
        assert_eq!(fs::read_to_string(&path).unwrap(), "another writer");
    }
    #[test]
    fn preserves_bom_crlf_and_grapheme_deletion() {
        let (_dir, path) = fixture("\u{feff}中文👩‍💻\r\n".as_bytes());
        let mut editor = FileEditor::open(&path, false).unwrap();
        editor.save().unwrap();
        assert_eq!(fs::read_to_string(&path).unwrap(), "\u{feff}中文👩‍💻\r\n");
        key(&mut editor, KeyCode::End, KeyModifiers::empty());
        key(&mut editor, KeyCode::Backspace, KeyModifiers::empty());
        editor.save().unwrap();
        assert_eq!(fs::read_to_string(path).unwrap(), "\u{feff}中文\r\n");
    }
    #[test]
    fn viewer_is_readonly_and_binary_is_hex() {
        let (_dir, path) = fixture(&[0xff, 0x00, 0x1b]);
        assert!(FileEditor::open(&path, false).is_err());
        let mut editor = FileEditor::open(&path, true).unwrap();
        editor.handle_event(Event::Paste("overwrite".into()));
        assert!(!editor.dirty());
        assert!(editor.state.text.contains("ff 00 1b"));
        assert!(editor.save().is_err());
        assert_eq!(display("\x1b[31m"), "�[31m");
    }
    #[test]
    fn navigation_search_and_resize_are_safe() {
        let (_dir, path) = fixture("first\r\n中文👩‍💻\r\nlast".as_bytes());
        let mut editor = FileEditor::open(&path, false).unwrap();
        editor.goto(2, 1);
        key(&mut editor, KeyCode::End, KeyModifiers::empty());
        key(&mut editor, KeyCode::Right, KeyModifiers::empty());
        assert_eq!(editor.state.cursor, "first\r\n中文👩‍💻\r\n".len());
        key(&mut editor, KeyCode::Left, KeyModifiers::empty());
        assert_eq!(editor.state.cursor, "first\r\n中文👩‍💻".len());
        key(&mut editor, KeyCode::Char('f'), KeyModifiers::CTRL);
        editor.handle_event(Event::Paste("first".into()));
        key(&mut editor, KeyCode::Enter, KeyModifiers::empty());
        assert_eq!(editor.state.cursor, 0);
        for (width, height) in [(1, 1), (4, 3), (100, 24)] {
            let mut terminal = Terminal::new(
                ratatui::backend::TestBackend::new(width, height),
            )
            .unwrap();
            terminal.draw(|frame| editor.draw(frame)).unwrap();
        }
    }
    #[test]
    fn debug_never_contains_document() {
        let (_dir, path) = fixture(b"secret-key");
        let editor = FileEditor::open(&path, false).unwrap();
        assert!(!format!("{editor:?}").contains("secret-key"));
    }
}
