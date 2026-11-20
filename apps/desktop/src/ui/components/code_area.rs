//! Éditeur de code multiligne : défilement, sélection à la souris et au clavier,
//! saisie composée (IME), presse-papiers, annuler/rétablir, indentation,
//! commentaire de ligne, coloration simple et marqueurs de problèmes dans la marge.
//!
//! Le texte est gardé en un seul `String` avec des `\n` ; les fins de ligne du
//! fichier sont restituées par l'appelant à l'enregistrement.

use std::ops::Range;

use gpui::{
    App, Bounds, ClipboardItem, ContentMask, Context, CursorStyle, ElementId, ElementInputHandler,
    Entity, EntityInputHandler, EventEmitter, FocusHandle, Focusable, Font, GlobalElementId, Hsla,
    KeyBinding, LayoutId, MouseButton, MouseDownEvent, MouseMoveEvent, MouseUpEvent, PaintQuad,
    Pixels, Point, ScrollWheelEvent, ShapedLine, SharedString, Style, TextRun, UTF16Selection,
    UnderlineStyle, Window, actions, div, fill, font, point, prelude::*, px, relative, size,
};
use unicode_segmentation::UnicodeSegmentation;

use crate::model::Severity;
use crate::theme::{Theme, fonts, metrics};

actions!(
    code_area,
    [
        Backspace,
        DeleteWordLeft,
        DeleteToLineStart,
        Delete,
        Left,
        Right,
        Up,
        Down,
        SelectLeft,
        SelectRight,
        SelectUp,
        SelectDown,
        WordLeft,
        WordRight,
        SelectWordLeft,
        SelectWordRight,
        LineStart,
        LineEnd,
        SelectLineStart,
        SelectLineEnd,
        DocStart,
        DocEnd,
        SelectDocStart,
        SelectDocEnd,
        PageUp,
        PageDown,
        SelectAll,
        Newline,
        Indent,
        Outdent,
        ToggleComment,
        DeleteLine,
        Copy,
        Cut,
        Paste,
        Undo,
        Redo,
        Save,
        Cancel,
        ShowCharacterPalette,
    ]
);

const KEY_CONTEXT: &str = "CodeArea";
const TAB_WIDTH: usize = 4;
const UNDO_LIMIT: usize = 200;
const CURSOR_WIDTH: Pixels = px(2.);
const MARKER_SIZE: Pixels = px(7.);
/// Largeur réservée au marqueur de problème, à gauche des numéros de ligne.
const MARKER_COLUMN: Pixels = px(16.);
const UNDERLINE_THICKNESS: Pixels = px(1.);
const RULE_WIDTH: Pixels = px(1.);
const ZERO: Pixels = px(0.);
/// Largeur d'un caractère si la police ne répond pas.
const FALLBACK_CHAR_WIDTH: Pixels = px(7.8);

/// Raccourcis de l'éditeur, à enregistrer une fois au démarrage.
pub fn bind_code_area_keys(cx: &mut App) {
    let c = Some(KEY_CONTEXT);
    cx.bind_keys([
        KeyBinding::new("backspace", Backspace, c),
        KeyBinding::new("shift-backspace", Backspace, c),
        KeyBinding::new("alt-backspace", DeleteWordLeft, c),
        KeyBinding::new("cmd-backspace", DeleteToLineStart, c),
        KeyBinding::new("delete", Delete, c),
        KeyBinding::new("left", Left, c),
        KeyBinding::new("right", Right, c),
        KeyBinding::new("up", Up, c),
        KeyBinding::new("down", Down, c),
        KeyBinding::new("shift-left", SelectLeft, c),
        KeyBinding::new("shift-right", SelectRight, c),
        KeyBinding::new("shift-up", SelectUp, c),
        KeyBinding::new("shift-down", SelectDown, c),
        KeyBinding::new("alt-left", WordLeft, c),
        KeyBinding::new("alt-right", WordRight, c),
        KeyBinding::new("alt-shift-left", SelectWordLeft, c),
        KeyBinding::new("alt-shift-right", SelectWordRight, c),
        KeyBinding::new("home", LineStart, c),
        KeyBinding::new("end", LineEnd, c),
        KeyBinding::new("cmd-left", LineStart, c),
        KeyBinding::new("cmd-right", LineEnd, c),
        KeyBinding::new("shift-home", SelectLineStart, c),
        KeyBinding::new("shift-end", SelectLineEnd, c),
        KeyBinding::new("cmd-shift-left", SelectLineStart, c),
        KeyBinding::new("cmd-shift-right", SelectLineEnd, c),
        KeyBinding::new("cmd-up", DocStart, c),
        KeyBinding::new("cmd-down", DocEnd, c),
        KeyBinding::new("cmd-shift-up", SelectDocStart, c),
        KeyBinding::new("cmd-shift-down", SelectDocEnd, c),
        KeyBinding::new("pageup", PageUp, c),
        KeyBinding::new("pagedown", PageDown, c),
        KeyBinding::new("cmd-a", SelectAll, c),
        KeyBinding::new("enter", Newline, c),
        KeyBinding::new("shift-enter", Newline, c),
        KeyBinding::new("tab", Indent, c),
        KeyBinding::new("shift-tab", Outdent, c),
        KeyBinding::new("cmd-]", Indent, c),
        KeyBinding::new("cmd-[", Outdent, c),
        KeyBinding::new("cmd-/", ToggleComment, c),
        KeyBinding::new("cmd-shift-k", DeleteLine, c),
        KeyBinding::new("cmd-c", Copy, c),
        KeyBinding::new("cmd-x", Cut, c),
        KeyBinding::new("cmd-v", Paste, c),
        KeyBinding::new("cmd-z", Undo, c),
        KeyBinding::new("cmd-shift-z", Redo, c),
        KeyBinding::new("cmd-s", Save, c),
        KeyBinding::new("escape", Cancel, c),
        KeyBinding::new("ctrl-cmd-space", ShowCharacterPalette, c),
    ]);
}

/// Événements émis vers l'écran qui contient l'éditeur.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CodeAreaEvent {
    /// Le texte a changé.
    Edited,
    /// Le curseur ou la sélection a bougé.
    CursorMoved,
    /// Cmd+S.
    Save,
}

/// Problème affiché dans la marge, sur une ligne (0-indexée).
#[derive(Clone, Debug)]
pub struct Diagnostic {
    pub row: usize,
    pub severity: Severity,
    pub message: SharedString,
}

// ---------------------------------------------------------------------------
// Texte

/// Texte et débuts de ligne. Positions en octets UTF-8.
#[derive(Clone, Debug)]
struct Buffer {
    text: String,
    line_starts: Vec<usize>,
    /// Plus longue ligne, en colonnes affichées (défilement horizontal).
    widest: usize,
}

impl Buffer {
    fn new(text: String) -> Self {
        let mut buffer = Self {
            text,
            line_starts: Vec::new(),
            widest: 0,
        };
        buffer.reindex();
        buffer
    }

    fn reindex(&mut self) {
        self.line_starts.clear();
        self.line_starts.push(0);
        self.line_starts.extend(
            self.text
                .bytes()
                .enumerate()
                .filter(|(_, b)| *b == b'\n')
                .map(|(i, _)| i + 1),
        );
        self.widest = (0..self.line_count())
            .map(|row| display_width(self.line(row)))
            .max()
            .unwrap_or(0);
    }

    fn line_count(&self) -> usize {
        self.line_starts.len()
    }

    /// Octets de la ligne, sans le `\n`.
    fn line_range(&self, row: usize) -> Range<usize> {
        let start = self.line_starts[row];
        let end = self
            .line_starts
            .get(row + 1)
            .map_or(self.text.len(), |next| next - 1);
        start..end
    }

    fn line(&self, row: usize) -> &str {
        &self.text[self.line_range(row)]
    }

    fn row_of(&self, offset: usize) -> usize {
        match self.line_starts.binary_search(&offset) {
            Ok(row) => row,
            Err(next) => next - 1,
        }
    }

    /// `(ligne, colonne en octets)`.
    fn point(&self, offset: usize) -> (usize, usize) {
        let row = self.row_of(offset);
        (row, offset - self.line_starts[row])
    }

    fn replace(&mut self, range: Range<usize>, new_text: &str) {
        let start = range.start;
        // Index des lignes mis à jour sur place : les débuts de ligne avant
        // l'édition restent, ceux du texte retiré disparaissent, ceux du texte
        // inséré s'ajoutent et les suivants sont décalés. Une frappe ne relit
        // plus tout le fichier.
        let first = self.row_of(range.start);
        let last = self.row_of(range.end);
        let removed = range.end - range.start;
        let shift = |offset: usize| offset + new_text.len() - removed;
        self.text.replace_range(range, new_text);
        let tail: Vec<usize> = self.line_starts[last + 1..]
            .iter()
            .map(|&offset| shift(offset))
            .collect();
        self.line_starts.truncate(first + 1);
        self.line_starts.extend(
            new_text
                .bytes()
                .enumerate()
                .filter(|(_, b)| *b == b'\n')
                .map(|(i, _)| start + i + 1),
        );
        self.line_starts.extend(tail);
        // Largeur maximale : seules les lignes touchées peuvent l'agrandir. Une
        // suppression peut la réduire ; on garde l'ancienne valeur (un peu de
        // défilement horizontal en trop, sans conséquence).
        let first = self.row_of(start);
        let last = self.row_of((start + new_text.len()).min(self.text.len()));
        for row in first..=last {
            self.widest = self.widest.max(display_width(self.line(row)));
        }
    }
}

/// Largeur affichée (tabulations développées) d'un texte sans saut de ligne.
fn display_width(line: &str) -> usize {
    line.chars()
        .map(|c| if c == '\t' { TAB_WIDTH } else { 1 })
        .sum()
}

/// Ligne affichée : tabulations remplacées par des espaces.
fn display_line(line: &str) -> String {
    line.replace('\t', &" ".repeat(TAB_WIDTH))
}

/// Octet affiché qui correspond à l'octet `col` de la ligne.
fn display_offset(line: &str, col: usize) -> usize {
    line[..col.min(line.len())]
        .chars()
        .map(|c| if c == '\t' { TAB_WIDTH } else { c.len_utf8() })
        .sum()
}

/// Octet de la ligne qui correspond à l'octet affiché `display`.
fn content_offset(line: &str, display: usize) -> usize {
    let mut shown = 0;
    for (ix, c) in line.char_indices() {
        let width = if c == '\t' { TAB_WIDTH } else { c.len_utf8() };
        if shown + width > display {
            // Au milieu d'une tabulation : la plus proche des deux bornes.
            return if display - shown > width / 2 && c == '\t' {
                ix + c.len_utf8()
            } else {
                ix
            };
        }
        shown += width;
    }
    line.len()
}

/// Colonne affichée en caractères (déplacements verticaux).
fn display_column(line: &str, col: usize) -> usize {
    display_width(&line[..col.min(line.len())])
}

/// Octet de la ligne le plus proche de la colonne affichée `column`.
fn offset_for_column(line: &str, column: usize) -> usize {
    let mut shown = 0;
    for (ix, c) in line.char_indices() {
        let width = if c == '\t' { TAB_WIDTH } else { 1 };
        if shown + width > column {
            return ix;
        }
        shown += width;
    }
    line.len()
}

fn leading_whitespace(line: &str) -> &str {
    let end = line.len() - line.trim_start_matches([' ', '\t']).len();
    &line[..end]
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum CharKind {
    Space,
    Word,
    Punct,
}

fn char_kind(c: char) -> CharKind {
    if c.is_whitespace() {
        CharKind::Space
    } else if c.is_alphanumeric() || c == '_' {
        CharKind::Word
    } else {
        CharKind::Punct
    }
}

// ---------------------------------------------------------------------------
// Coloration

/// Coloration simple, ligne par ligne : commentaires, chaînes, nombres, mots-clés.
#[derive(Clone, Debug, Default)]
struct Syntax {
    line_comment: &'static str,
    keywords: &'static [&'static str],
}

const KEYWORDS: &[&str] = &[
    "abstract",
    "and",
    "as",
    "async",
    "await",
    "break",
    "case",
    "catch",
    "class",
    "const",
    "continue",
    "crate",
    "def",
    "default",
    "defer",
    "del",
    "do",
    "elif",
    "else",
    "enum",
    "except",
    "export",
    "extends",
    "false",
    "False",
    "final",
    "finally",
    "fn",
    "for",
    "from",
    "func",
    "function",
    "go",
    "if",
    "impl",
    "implements",
    "import",
    "in",
    "interface",
    "is",
    "let",
    "lambda",
    "loop",
    "match",
    "mod",
    "module",
    "mut",
    "new",
    "nil",
    "None",
    "not",
    "null",
    "or",
    "package",
    "pass",
    "private",
    "protected",
    "pub",
    "public",
    "raise",
    "return",
    "self",
    "Self",
    "static",
    "struct",
    "super",
    "switch",
    "this",
    "throw",
    "throws",
    "trait",
    "true",
    "True",
    "try",
    "type",
    "typeof",
    "use",
    "val",
    "var",
    "void",
    "where",
    "while",
    "with",
    "yield",
];

impl Syntax {
    fn for_path(path: &str) -> Self {
        let name = path.rsplit('/').next().unwrap_or(path).to_ascii_lowercase();
        let ext = name.rsplit_once('.').map_or("", |(_, ext)| ext);
        let hash = matches!(
            ext,
            "py" | "rb"
                | "sh"
                | "bash"
                | "zsh"
                | "yml"
                | "yaml"
                | "toml"
                | "pl"
                | "r"
                | "ex"
                | "exs"
                | "conf"
                | "ini"
                | "mk"
        ) || name == "dockerfile"
            || name == "makefile"
            || name.starts_with(".env");
        let plain = matches!(
            ext,
            "md" | "txt" | "json" | "lock" | "csv" | "svg" | "xml" | "html"
        );
        Self {
            line_comment: if plain {
                ""
            } else if hash {
                "#"
            } else if matches!(ext, "sql" | "lua" | "hs") {
                "--"
            } else {
                "//"
            },
            keywords: if plain { &[] } else { KEYWORDS },
        }
    }

    /// Plages `(octets affichés, couleur)` d'une ligne affichée.
    fn highlight(&self, line: &str, theme: &Theme) -> Vec<(Range<usize>, Hsla)> {
        let mut spans = Vec::new();
        let bytes = line.as_bytes();
        let mut i = 0;
        while i < line.len() {
            let rest = &line[i..];
            if !self.line_comment.is_empty() && rest.starts_with(self.line_comment) {
                spans.push((i..line.len(), theme.code_comment));
                break;
            }
            if rest.starts_with("/*") && self.line_comment == "//" {
                let end = rest.find("*/").map_or(line.len(), |e| i + e + 2);
                spans.push((i..end, theme.code_comment));
                i = end;
                continue;
            }
            let c = rest.chars().next().unwrap_or(' ');
            if matches!(c, '"' | '\'' | '`') && !self.keywords.is_empty() {
                // Apostrophe dans un mot (durée de vie Rust, anglais) : pas une chaîne.
                let after_word =
                    i > 0 && char_kind(line[..i].chars().last().unwrap_or(' ')) == CharKind::Word;
                if !(c == '\'' && after_word) {
                    let mut end = i + 1;
                    let mut escaped = false;
                    while end < line.len() {
                        let b = bytes[end];
                        if escaped {
                            escaped = false;
                        } else if b == b'\\' {
                            escaped = true;
                        } else if b == c as u8 {
                            end += 1;
                            break;
                        }
                        end += 1;
                    }
                    let end = end.min(line.len());
                    // Repli sur une frontière de caractère.
                    let end = (end..=line.len())
                        .find(|e| line.is_char_boundary(*e))
                        .unwrap_or(line.len());
                    spans.push((i..end, theme.code_string));
                    i = end;
                    continue;
                }
            }
            if char_kind(c) == CharKind::Word {
                let len = rest
                    .char_indices()
                    .find(|(_, ch)| char_kind(*ch) != CharKind::Word)
                    .map_or(rest.len(), |(ix, _)| ix);
                let word = &rest[..len];
                let starts_with_digit = word.chars().next().is_some_and(|d| d.is_ascii_digit());
                if starts_with_digit {
                    spans.push((i..i + len, theme.code_number));
                } else if self.keywords.contains(&word) {
                    spans.push((i..i + len, theme.code_keyword));
                }
                i += len;
                continue;
            }
            i += c.len_utf8();
        }
        spans
    }
}

// ---------------------------------------------------------------------------
// Éditeur

#[derive(Clone)]
struct Snapshot {
    text: String,
    selection: Range<usize>,
    diagnostics: Vec<Diagnostic>,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum EditKind {
    None,
    Typing,
    Deleting,
    Other,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Autoscroll {
    Fit,
    Center,
}

/// Géométrie du dernier rendu, pour la souris et l'IME.
#[derive(Clone)]
struct Layout {
    bounds: Bounds<Pixels>,
    /// Bord gauche du texte (après la marge).
    text_left: Pixels,
    line_height: Pixels,
    char_width: Pixels,
    lines: Vec<(usize, ShapedLine)>,
}

pub struct CodeArea {
    focus_handle: FocusHandle,
    buffer: Buffer,
    selected: Range<usize>,
    reversed: bool,
    marked: Option<Range<usize>>,
    /// Colonne visée par les déplacements verticaux.
    goal_column: Option<usize>,
    scroll: Point<Pixels>,
    autoscroll: Option<Autoscroll>,
    diagnostics: Vec<Diagnostic>,
    syntax: Syntax,
    /// Unité d'indentation du fichier (`"\t"` ou des espaces).
    indent: String,
    undo: Vec<Snapshot>,
    redo: Vec<Snapshot>,
    last_edit: EditKind,
    version: u64,
    saved_version: u64,
    layout: Option<Layout>,
    is_selecting: bool,
}

impl EventEmitter<CodeAreaEvent> for CodeArea {}

impl CodeArea {
    /// `path` choisit la coloration et le style de commentaire.
    pub fn new(text: &str, path: &str, cx: &mut Context<Self>) -> Self {
        let text = text.replace("\r\n", "\n");
        let indent = detect_indent(&text);
        Self {
            focus_handle: cx.focus_handle(),
            buffer: Buffer::new(text),
            selected: 0..0,
            reversed: false,
            marked: None,
            goal_column: None,
            scroll: Point::default(),
            autoscroll: None,
            diagnostics: Vec::new(),
            syntax: Syntax::for_path(path),
            indent,
            undo: Vec::new(),
            redo: Vec::new(),
            last_edit: EditKind::None,
            version: 0,
            saved_version: 0,
            layout: None,
            is_selecting: false,
        }
    }

    pub fn text(&self) -> &str {
        &self.buffer.text
    }

    pub fn line_count(&self) -> usize {
        self.buffer.line_count()
    }

    pub fn is_dirty(&self) -> bool {
        self.version != self.saved_version
    }

    pub fn mark_saved(&mut self, cx: &mut Context<Self>) {
        self.saved_version = self.version;
        self.last_edit = EditKind::None;
        cx.emit(CodeAreaEvent::Edited);
    }

    pub fn set_diagnostics(&mut self, diagnostics: Vec<Diagnostic>, cx: &mut Context<Self>) {
        self.diagnostics = diagnostics;
        cx.notify();
    }

    pub fn diagnostics(&self) -> &[Diagnostic] {
        &self.diagnostics
    }

    /// Ligne et colonne (1-indexées) du curseur.
    pub fn cursor_position(&self) -> (usize, usize) {
        let (row, col) = self.buffer.point(self.cursor());
        let column = self.buffer.line(row)[..col].chars().count();
        (row + 1, column + 1)
    }

    /// Nombre de caractères sélectionnés.
    pub fn selection_len(&self) -> usize {
        self.buffer.text[self.selected.clone()].chars().count()
    }

    /// Place le curseur au début du code de la ligne (1-indexée), centrée à l'écran.
    pub fn go_to_line(&mut self, line: usize, cx: &mut Context<Self>) {
        let row = line.saturating_sub(1).min(self.buffer.line_count() - 1);
        let text = self.buffer.line(row);
        let offset = self.buffer.line_starts[row] + leading_whitespace(text).len();
        self.select_range(offset..offset, false);
        self.autoscroll = Some(Autoscroll::Center);
        self.goal_column = None;
        cx.emit(CodeAreaEvent::CursorMoved);
        cx.notify();
    }

    // -- sélection ----------------------------------------------------------

    fn cursor(&self) -> usize {
        if self.reversed {
            self.selected.start
        } else {
            self.selected.end
        }
    }

    fn anchor(&self) -> usize {
        if self.reversed {
            self.selected.end
        } else {
            self.selected.start
        }
    }

    fn select_range(&mut self, range: Range<usize>, reversed: bool) {
        self.selected = range;
        self.reversed = reversed;
    }

    /// Déplace le curseur ; avec `extend`, étend la sélection depuis l'ancre.
    fn move_cursor(&mut self, offset: usize, extend: bool, cx: &mut Context<Self>) {
        let offset = offset.min(self.buffer.text.len());
        if extend {
            let anchor = self.anchor();
            if offset < anchor {
                self.select_range(offset..anchor, true);
            } else {
                self.select_range(anchor..offset, false);
            }
        } else {
            self.select_range(offset..offset, false);
        }
        self.last_edit = EditKind::None;
        self.autoscroll = Some(Autoscroll::Fit);
        cx.emit(CodeAreaEvent::CursorMoved);
        cx.notify();
    }

    fn previous_boundary(&self, offset: usize) -> usize {
        self.buffer.text[..offset]
            .grapheme_indices(true)
            .next_back()
            .map_or(0, |(ix, _)| ix)
    }

    fn next_boundary(&self, offset: usize) -> usize {
        self.buffer.text[offset..]
            .graphemes(true)
            .next()
            .map_or(self.buffer.text.len(), |g| offset + g.len())
    }

    fn word_left(&self, offset: usize) -> usize {
        let before = &self.buffer.text[..offset];
        let mut chars = before.char_indices().rev().peekable();
        // Espaces (sans franchir plus d'un saut de ligne), puis un groupe homogène.
        while let Some((_, c)) = chars.peek() {
            if c.is_whitespace() && *c != '\n' {
                chars.next();
            } else {
                break;
            }
        }
        let Some(&(ix, first)) = chars.peek() else {
            return 0;
        };
        if first == '\n' {
            return ix;
        }
        let kind = char_kind(first);
        let mut start = ix;
        for (ix, c) in chars {
            if char_kind(c) != kind || c == '\n' {
                break;
            }
            start = ix;
        }
        start
    }

    fn word_right(&self, offset: usize) -> usize {
        let after = &self.buffer.text[offset..];
        let mut chars = after.char_indices().peekable();
        while let Some((_, c)) = chars.peek() {
            if c.is_whitespace() && *c != '\n' {
                chars.next();
            } else {
                break;
            }
        }
        let Some(&(ix, first)) = chars.peek() else {
            return self.buffer.text.len();
        };
        if first == '\n' {
            return offset + ix + 1;
        }
        let kind = char_kind(first);
        let mut end = offset + after.len();
        for (ix, c) in chars {
            if char_kind(c) != kind || c == '\n' {
                end = offset + ix;
                break;
            }
        }
        end
    }

    fn vertical_target(&mut self, rows: isize) -> usize {
        let (row, col) = self.buffer.point(self.cursor());
        let column = *self
            .goal_column
            .get_or_insert_with(|| display_column(self.buffer.line(row), col));
        let target = row as isize + rows;
        if target < 0 {
            return 0;
        }
        let target = target as usize;
        if target >= self.buffer.line_count() {
            return self.buffer.text.len();
        }
        let line = self.buffer.line(target);
        self.buffer.line_starts[target] + offset_for_column(line, column)
    }

    fn move_vertically(&mut self, rows: isize, extend: bool, cx: &mut Context<Self>) {
        if !extend && !self.selected.is_empty() && rows.abs() == 1 {
            let edge = if rows < 0 {
                self.selected.start
            } else {
                self.selected.end
            };
            self.select_range(edge..edge, false);
        }
        let goal = self.goal_column;
        let target = self.vertical_target(rows);
        let goal = goal.or(self.goal_column);
        self.move_cursor(target, extend, cx);
        self.goal_column = goal;
    }

    fn line_start_target(&self) -> usize {
        // Début intelligent : début du code, puis colonne 0.
        let (row, col) = self.buffer.point(self.cursor());
        let indent = leading_whitespace(self.buffer.line(row)).len();
        let start = self.buffer.line_starts[row];
        if col == indent { start } else { start + indent }
    }

    fn line_end_target(&self) -> usize {
        let row = self.buffer.row_of(self.cursor());
        self.buffer.line_range(row).end
    }

    fn page_rows(&self) -> isize {
        self.layout.as_ref().map_or(20, |layout| {
            ((layout.bounds.size.height / layout.line_height).floor() as isize - 2).max(1)
        })
    }

    // -- modifications -------------------------------------------------------

    fn snapshot(&self) -> Snapshot {
        Snapshot {
            text: self.buffer.text.clone(),
            selection: self.selected.clone(),
            diagnostics: self.diagnostics.clone(),
        }
    }

    fn push_undo(&mut self, kind: EditKind) {
        let coalesce = kind != EditKind::Other && kind == self.last_edit;
        if !coalesce {
            self.undo.push(self.snapshot());
            if self.undo.len() > UNDO_LIMIT {
                self.undo.remove(0);
            }
        }
        self.redo.clear();
        self.last_edit = kind;
    }

    /// Remplace `range` par `new_text`, curseur après le texte inséré.
    fn edit(
        &mut self,
        range: Range<usize>,
        new_text: &str,
        kind: EditKind,
        cx: &mut Context<Self>,
    ) {
        let len = self.buffer.text.len();
        let range = range.start.min(len)..range.end.min(len);
        if range.is_empty() && new_text.is_empty() {
            return;
        }
        self.push_undo(kind);
        self.shift_diagnostics(&range, new_text);
        self.buffer.replace(range.clone(), new_text);
        let cursor = range.start + new_text.len();
        self.select_range(cursor..cursor, false);
        self.marked = None;
        self.goal_column = None;
        self.version += 1;
        self.autoscroll = Some(Autoscroll::Fit);
        cx.emit(CodeAreaEvent::Edited);
        cx.notify();
    }

    /// Garde les marqueurs sur leurs lignes quand des lignes sont ajoutées ou retirées au-dessus.
    fn shift_diagnostics(&mut self, range: &Range<usize>, new_text: &str) {
        let start_row = self.buffer.row_of(range.start);
        let removed = self.buffer.text[range.clone()].matches('\n').count();
        let inserted = new_text.matches('\n').count();
        if removed == 0 && inserted == 0 {
            return;
        }
        for diagnostic in &mut self.diagnostics {
            if diagnostic.row > start_row + removed {
                diagnostic.row = diagnostic.row + inserted - removed;
            } else if diagnostic.row > start_row {
                diagnostic.row = start_row + inserted.min(diagnostic.row - start_row);
            }
        }
    }

    fn restore(&mut self, snapshot: Snapshot, cx: &mut Context<Self>) {
        self.buffer = Buffer::new(snapshot.text);
        let end = snapshot.selection.end.min(self.buffer.text.len());
        let start = snapshot.selection.start.min(end);
        self.select_range(start..end, false);
        self.diagnostics = snapshot.diagnostics;
        self.marked = None;
        self.last_edit = EditKind::None;
        self.version += 1;
        self.autoscroll = Some(Autoscroll::Fit);
        cx.emit(CodeAreaEvent::Edited);
        cx.notify();
    }

    /// Lignes touchées par la sélection (la dernière exclue si la sélection s'arrête à sa colonne 0).
    fn selected_rows(&self) -> Range<usize> {
        let first = self.buffer.row_of(self.selected.start);
        let (last, col) = self.buffer.point(self.selected.end);
        let last = if col == 0 && last > first {
            last - 1
        } else {
            last
        };
        first..last + 1
    }

    /// Réécrit des lignes entières et garde une sélection équivalente.
    fn rewrite_rows(
        &mut self,
        rows: Range<usize>,
        transform: impl Fn(&str) -> String,
        cx: &mut Context<Self>,
    ) {
        let start = self.buffer.line_starts[rows.start];
        let end = self.buffer.line_range(rows.end - 1).end;
        let old = self.buffer.text[start..end].to_owned();
        let new = old
            .split('\n')
            .map(&transform)
            .collect::<Vec<_>>()
            .join("\n");
        if new == old {
            return;
        }
        let had_selection = !self.selected.is_empty();
        let (cursor_row, cursor_col) = self.buffer.point(self.cursor());
        let old_line_len = self.buffer.line(cursor_row).len();
        self.edit(start..end, &new, EditKind::Other, cx);
        if had_selection {
            let end = start + new.len();
            self.select_range(start..end, false);
        } else {
            let line = self.buffer.line_range(cursor_row);
            let delta = line.len() as isize - old_line_len as isize;
            let col = (cursor_col as isize + delta).clamp(0, line.len() as isize) as usize;
            let offset = line.start + col;
            self.select_range(offset..offset, false);
        }
        cx.notify();
    }

    // -- actions -------------------------------------------------------------

    fn backspace(&mut self, _: &Backspace, _: &mut Window, cx: &mut Context<Self>) {
        if !self.selected.is_empty() {
            return self.edit(self.selected.clone(), "", EditKind::Other, cx);
        }
        let cursor = self.cursor();
        let (row, col) = self.buffer.point(cursor);
        let line = self.buffer.line(row);
        // Dans l'indentation faite d'espaces : retire une unité d'indentation.
        let start = if col > 0 && self.indent != "\t" && line[..col].bytes().all(|b| b == b' ') {
            let unit = self.indent.len().max(1);
            cursor - (((col - 1) % unit) + 1)
        } else {
            self.previous_boundary(cursor)
        };
        self.edit(start..cursor, "", EditKind::Deleting, cx);
    }

    fn delete_word_left(&mut self, _: &DeleteWordLeft, _: &mut Window, cx: &mut Context<Self>) {
        if !self.selected.is_empty() {
            return self.edit(self.selected.clone(), "", EditKind::Other, cx);
        }
        let cursor = self.cursor();
        let start = self.word_left(cursor);
        self.edit(start..cursor, "", EditKind::Other, cx);
    }

    fn delete_to_line_start(
        &mut self,
        _: &DeleteToLineStart,
        _: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if !self.selected.is_empty() {
            return self.edit(self.selected.clone(), "", EditKind::Other, cx);
        }
        let cursor = self.cursor();
        let row = self.buffer.row_of(cursor);
        let start = self.buffer.line_starts[row];
        let start = if start == cursor {
            self.previous_boundary(cursor)
        } else {
            start
        };
        self.edit(start..cursor, "", EditKind::Other, cx);
    }

    fn delete(&mut self, _: &Delete, _: &mut Window, cx: &mut Context<Self>) {
        if !self.selected.is_empty() {
            return self.edit(self.selected.clone(), "", EditKind::Other, cx);
        }
        let cursor = self.cursor();
        let end = self.next_boundary(cursor);
        self.edit(cursor..end, "", EditKind::Deleting, cx);
    }

    fn left(&mut self, _: &Left, _: &mut Window, cx: &mut Context<Self>) {
        let target = if self.selected.is_empty() {
            self.previous_boundary(self.cursor())
        } else {
            self.selected.start
        };
        self.move_cursor(target, false, cx);
        self.goal_column = None;
    }

    fn right(&mut self, _: &Right, _: &mut Window, cx: &mut Context<Self>) {
        let target = if self.selected.is_empty() {
            self.next_boundary(self.cursor())
        } else {
            self.selected.end
        };
        self.move_cursor(target, false, cx);
        self.goal_column = None;
    }

    fn up(&mut self, _: &Up, _: &mut Window, cx: &mut Context<Self>) {
        self.move_vertically(-1, false, cx);
    }

    fn down(&mut self, _: &Down, _: &mut Window, cx: &mut Context<Self>) {
        self.move_vertically(1, false, cx);
    }

    fn select_left(&mut self, _: &SelectLeft, _: &mut Window, cx: &mut Context<Self>) {
        let target = self.previous_boundary(self.cursor());
        self.move_cursor(target, true, cx);
        self.goal_column = None;
    }

    fn select_right(&mut self, _: &SelectRight, _: &mut Window, cx: &mut Context<Self>) {
        let target = self.next_boundary(self.cursor());
        self.move_cursor(target, true, cx);
        self.goal_column = None;
    }

    fn select_up(&mut self, _: &SelectUp, _: &mut Window, cx: &mut Context<Self>) {
        self.move_vertically(-1, true, cx);
    }

    fn select_down(&mut self, _: &SelectDown, _: &mut Window, cx: &mut Context<Self>) {
        self.move_vertically(1, true, cx);
    }

    fn word_left_action(&mut self, _: &WordLeft, _: &mut Window, cx: &mut Context<Self>) {
        let target = self.word_left(self.cursor());
        self.move_cursor(target, false, cx);
        self.goal_column = None;
    }

    fn word_right_action(&mut self, _: &WordRight, _: &mut Window, cx: &mut Context<Self>) {
        let target = self.word_right(self.cursor());
        self.move_cursor(target, false, cx);
        self.goal_column = None;
    }

    fn select_word_left(&mut self, _: &SelectWordLeft, _: &mut Window, cx: &mut Context<Self>) {
        let target = self.word_left(self.cursor());
        self.move_cursor(target, true, cx);
        self.goal_column = None;
    }

    fn select_word_right(&mut self, _: &SelectWordRight, _: &mut Window, cx: &mut Context<Self>) {
        let target = self.word_right(self.cursor());
        self.move_cursor(target, true, cx);
        self.goal_column = None;
    }

    fn line_start(&mut self, _: &LineStart, _: &mut Window, cx: &mut Context<Self>) {
        let target = self.line_start_target();
        self.move_cursor(target, false, cx);
        self.goal_column = None;
    }

    fn line_end(&mut self, _: &LineEnd, _: &mut Window, cx: &mut Context<Self>) {
        let target = self.line_end_target();
        self.move_cursor(target, false, cx);
        self.goal_column = None;
    }

    fn select_line_start(&mut self, _: &SelectLineStart, _: &mut Window, cx: &mut Context<Self>) {
        let target = self.line_start_target();
        self.move_cursor(target, true, cx);
        self.goal_column = None;
    }

    fn select_line_end(&mut self, _: &SelectLineEnd, _: &mut Window, cx: &mut Context<Self>) {
        let target = self.line_end_target();
        self.move_cursor(target, true, cx);
        self.goal_column = None;
    }

    fn doc_start(&mut self, _: &DocStart, _: &mut Window, cx: &mut Context<Self>) {
        self.move_cursor(0, false, cx);
        self.goal_column = None;
    }

    fn doc_end(&mut self, _: &DocEnd, _: &mut Window, cx: &mut Context<Self>) {
        self.move_cursor(self.buffer.text.len(), false, cx);
        self.goal_column = None;
    }

    fn select_doc_start(&mut self, _: &SelectDocStart, _: &mut Window, cx: &mut Context<Self>) {
        self.move_cursor(0, true, cx);
    }

    fn select_doc_end(&mut self, _: &SelectDocEnd, _: &mut Window, cx: &mut Context<Self>) {
        self.move_cursor(self.buffer.text.len(), true, cx);
    }

    fn page_up(&mut self, _: &PageUp, _: &mut Window, cx: &mut Context<Self>) {
        let rows = self.page_rows();
        self.move_vertically(-rows, false, cx);
    }

    fn page_down(&mut self, _: &PageDown, _: &mut Window, cx: &mut Context<Self>) {
        let rows = self.page_rows();
        self.move_vertically(rows, false, cx);
    }

    fn select_all(&mut self, _: &SelectAll, _: &mut Window, cx: &mut Context<Self>) {
        self.select_range(0..self.buffer.text.len(), false);
        cx.emit(CodeAreaEvent::CursorMoved);
        cx.notify();
    }

    fn newline(&mut self, _: &Newline, _: &mut Window, cx: &mut Context<Self>) {
        let cursor = self.selected.start;
        let (row, col) = self.buffer.point(cursor);
        let line = self.buffer.line(row);
        let mut indent = leading_whitespace(line).to_owned();
        indent.truncate(indent.len().min(col));
        let before = line[..col].trim_end();
        if before.ends_with(['{', '(', '['])
            || (self.syntax.line_comment == "#" && before.ends_with(':'))
        {
            indent.push_str(&self.indent);
        }
        let text = format!("\n{indent}");
        self.edit(self.selected.clone(), &text, EditKind::Other, cx);
    }

    fn indent_action(&mut self, _: &Indent, _: &mut Window, cx: &mut Context<Self>) {
        let rows = self.selected_rows();
        if self.selected.is_empty() || rows.len() == 1 && !self.selection_spans_line() {
            let unit = if self.indent == "\t" {
                "\t".to_owned()
            } else {
                let (row, col) = self.buffer.point(self.cursor());
                let column = display_column(self.buffer.line(row), col);
                let width = self.indent.len().max(1);
                " ".repeat(width - column % width)
            };
            return self.edit(self.selected.clone(), &unit, EditKind::Other, cx);
        }
        let unit = self.indent.clone();
        self.rewrite_rows(
            rows,
            |line| {
                if line.trim().is_empty() {
                    line.to_owned()
                } else {
                    format!("{unit}{line}")
                }
            },
            cx,
        );
    }

    fn selection_spans_line(&self) -> bool {
        self.buffer.text[self.selected.clone()].contains('\n')
    }

    fn outdent(&mut self, _: &Outdent, _: &mut Window, cx: &mut Context<Self>) {
        let rows = self.selected_rows();
        let width = self.indent.len().max(1);
        self.rewrite_rows(
            rows,
            |line| {
                if let Some(rest) = line.strip_prefix('\t') {
                    return rest.to_owned();
                }
                let spaces = line.len() - line.trim_start_matches(' ').len();
                line[spaces.min(width)..].to_owned()
            },
            cx,
        );
    }

    fn toggle_comment(&mut self, _: &ToggleComment, _: &mut Window, cx: &mut Context<Self>) {
        let prefix = self.syntax.line_comment;
        if prefix.is_empty() {
            return;
        }
        let rows = self.selected_rows();
        let all_commented = rows.clone().all(|row| {
            let line = self.buffer.line(row).trim_start();
            line.is_empty() || line.starts_with(prefix)
        });
        // Commentaire posé à l'indentation la plus faible du bloc.
        let column = rows
            .clone()
            .filter(|&row| !self.buffer.line(row).trim().is_empty())
            .map(|row| leading_whitespace(self.buffer.line(row)).len())
            .min()
            .unwrap_or(0);
        self.rewrite_rows(
            rows,
            |line| {
                if line.trim().is_empty() {
                    return line.to_owned();
                }
                if all_commented {
                    let indent = leading_whitespace(line);
                    let rest = &line[indent.len()..];
                    let rest = rest.strip_prefix(prefix).unwrap_or(rest);
                    let rest = rest.strip_prefix(' ').unwrap_or(rest);
                    format!("{indent}{rest}")
                } else {
                    let at = column.min(line.len());
                    format!("{}{prefix} {}", &line[..at], &line[at..])
                }
            },
            cx,
        );
    }

    fn delete_line(&mut self, _: &DeleteLine, _: &mut Window, cx: &mut Context<Self>) {
        let rows = self.selected_rows();
        let start = self.buffer.line_starts[rows.start];
        let end = self
            .buffer
            .line_starts
            .get(rows.end)
            .copied()
            .unwrap_or(self.buffer.text.len());
        // Dernière ligne : retire aussi le saut de ligne qui la précède.
        let start = if end == self.buffer.text.len() && start > 0 {
            start - 1
        } else {
            start
        };
        self.edit(start..end, "", EditKind::Other, cx);
    }

    /// Texte copié : la sélection, ou la ligne entière sans sélection.
    fn copied_range(&self) -> (Range<usize>, bool) {
        if !self.selected.is_empty() {
            return (self.selected.clone(), false);
        }
        let row = self.buffer.row_of(self.cursor());
        let start = self.buffer.line_starts[row];
        let end = self
            .buffer
            .line_starts
            .get(row + 1)
            .copied()
            .unwrap_or(self.buffer.text.len());
        (start..end, true)
    }

    fn copy(&mut self, _: &Copy, _: &mut Window, cx: &mut Context<Self>) {
        let (range, whole_line) = self.copied_range();
        let mut text = self.buffer.text[range].to_owned();
        if whole_line && !text.ends_with('\n') {
            text.push('\n');
        }
        cx.write_to_clipboard(ClipboardItem::new_string(text));
    }

    fn cut(&mut self, _: &Cut, _: &mut Window, cx: &mut Context<Self>) {
        let (range, whole_line) = self.copied_range();
        let mut text = self.buffer.text[range.clone()].to_owned();
        if whole_line && !text.ends_with('\n') {
            text.push('\n');
        }
        cx.write_to_clipboard(ClipboardItem::new_string(text));
        self.edit(range, "", EditKind::Other, cx);
    }

    fn paste(&mut self, _: &Paste, _: &mut Window, cx: &mut Context<Self>) {
        let Some(text) = cx.read_from_clipboard().and_then(|item| item.text()) else {
            return;
        };
        let text = text.replace("\r\n", "\n");
        self.edit(self.selected.clone(), &text, EditKind::Other, cx);
    }

    fn undo_action(&mut self, _: &Undo, _: &mut Window, cx: &mut Context<Self>) {
        if let Some(snapshot) = self.undo.pop() {
            self.redo.push(self.snapshot());
            self.restore(snapshot, cx);
        }
    }

    fn redo_action(&mut self, _: &Redo, _: &mut Window, cx: &mut Context<Self>) {
        if let Some(snapshot) = self.redo.pop() {
            self.undo.push(self.snapshot());
            self.restore(snapshot, cx);
        }
    }

    fn save(&mut self, _: &Save, _: &mut Window, cx: &mut Context<Self>) {
        cx.emit(CodeAreaEvent::Save);
    }

    fn cancel(&mut self, _: &Cancel, _: &mut Window, cx: &mut Context<Self>) {
        if !self.selected.is_empty() {
            let cursor = self.cursor();
            self.move_cursor(cursor, false, cx);
        }
    }

    fn show_character_palette(
        &mut self,
        _: &ShowCharacterPalette,
        window: &mut Window,
        _: &mut Context<Self>,
    ) {
        window.show_character_palette();
    }

    // -- souris --------------------------------------------------------------

    fn offset_for_position(&self, position: Point<Pixels>) -> Option<usize> {
        let layout = self.layout.as_ref()?;
        let y = position.y - layout.bounds.top() + self.scroll.y;
        let row =
            ((y / layout.line_height).floor().max(0.) as usize).min(self.buffer.line_count() - 1);
        let x = position.x - layout.text_left + self.scroll.x;
        let line = self.buffer.line(row);
        let display = match layout.lines.iter().find(|(r, _)| *r == row) {
            Some((_, shaped)) => shaped.closest_index_for_x(x.max(ZERO)),
            None => {
                let column = (x / layout.char_width).round().max(0.) as usize;
                display_offset(line, offset_for_column(line, column))
            }
        };
        Some(self.buffer.line_starts[row] + content_offset(line, display))
    }

    fn on_mouse_down(
        &mut self,
        event: &MouseDownEvent,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        window.focus(&self.focus_handle);
        let Some(offset) = self.offset_for_position(event.position) else {
            return;
        };
        match event.click_count {
            2 => {
                let start = self.word_left(self.next_boundary(offset).min(self.buffer.text.len()));
                let start = start.min(offset);
                let end = self.word_right(start).max(offset);
                self.select_range(start..end, false);
                cx.emit(CodeAreaEvent::CursorMoved);
                cx.notify();
            }
            3 => {
                let row = self.buffer.row_of(offset);
                let start = self.buffer.line_starts[row];
                let end = self
                    .buffer
                    .line_starts
                    .get(row + 1)
                    .copied()
                    .unwrap_or(self.buffer.text.len());
                self.select_range(start..end, false);
                cx.emit(CodeAreaEvent::CursorMoved);
                cx.notify();
            }
            _ => {
                self.is_selecting = true;
                self.move_cursor(offset, event.modifiers.shift, cx);
                self.goal_column = None;
            }
        }
    }

    fn on_mouse_up(&mut self, _: &MouseUpEvent, _: &mut Window, _: &mut Context<Self>) {
        self.is_selecting = false;
    }

    fn on_mouse_move(&mut self, event: &MouseMoveEvent, _: &mut Window, cx: &mut Context<Self>) {
        if !self.is_selecting {
            return;
        }
        if let Some(offset) = self.offset_for_position(event.position) {
            self.move_cursor(offset, true, cx);
        }
    }

    fn on_scroll(&mut self, event: &ScrollWheelEvent, _: &mut Window, cx: &mut Context<Self>) {
        let Some(layout) = &self.layout else {
            return;
        };
        let delta = event.delta.pixel_delta(layout.line_height);
        let max_y = layout.line_height * self.buffer.line_count().saturating_sub(1) as f32;
        let text_width = layout.bounds.right() - layout.text_left;
        let content_width = layout.char_width * (self.buffer.widest + 4) as f32;
        let max_x = (content_width - text_width).max(ZERO);
        self.scroll.y = (self.scroll.y - delta.y).clamp(ZERO, max_y);
        self.scroll.x = (self.scroll.x - delta.x).clamp(ZERO, max_x);
        cx.stop_propagation();
        cx.notify();
    }

    // -- UTF-16 (IME) --------------------------------------------------------

    fn offset_from_utf16(&self, offset: usize) -> usize {
        let mut utf8 = 0;
        let mut utf16 = 0;
        for ch in self.buffer.text.chars() {
            if utf16 >= offset {
                break;
            }
            utf16 += ch.len_utf16();
            utf8 += ch.len_utf8();
        }
        utf8
    }

    fn offset_to_utf16(&self, offset: usize) -> usize {
        self.buffer.text[..offset.min(self.buffer.text.len())]
            .chars()
            .map(char::len_utf16)
            .sum()
    }

    fn range_to_utf16(&self, range: &Range<usize>) -> Range<usize> {
        self.offset_to_utf16(range.start)..self.offset_to_utf16(range.end)
    }

    fn range_from_utf16(&self, range: &Range<usize>) -> Range<usize> {
        self.offset_from_utf16(range.start)..self.offset_from_utf16(range.end)
    }
}

/// Tabulation si la majorité des lignes indentées commencent par une tabulation,
/// sinon la plus petite indentation en espaces (2 ou 4).
fn detect_indent(text: &str) -> String {
    let mut tabs = 0;
    let mut spaces = 0;
    let mut smallest = usize::MAX;
    for line in text.lines() {
        if line.starts_with('\t') {
            tabs += 1;
        } else if line.starts_with(' ') && !line.trim().is_empty() {
            spaces += 1;
            let count = line.len() - line.trim_start_matches(' ').len();
            smallest = smallest.min(count);
        }
    }
    if tabs > spaces {
        "\t".to_owned()
    } else if smallest == 2 {
        "  ".to_owned()
    } else {
        " ".repeat(TAB_WIDTH)
    }
}

impl EntityInputHandler for CodeArea {
    fn text_for_range(
        &mut self,
        range_utf16: Range<usize>,
        actual_range: &mut Option<Range<usize>>,
        _: &mut Window,
        _: &mut Context<Self>,
    ) -> Option<String> {
        let range = self.range_from_utf16(&range_utf16);
        actual_range.replace(self.range_to_utf16(&range));
        Some(self.buffer.text[range].to_owned())
    }

    fn selected_text_range(
        &mut self,
        _: bool,
        _: &mut Window,
        _: &mut Context<Self>,
    ) -> Option<UTF16Selection> {
        Some(UTF16Selection {
            range: self.range_to_utf16(&self.selected),
            reversed: self.reversed,
        })
    }

    fn marked_text_range(&self, _: &mut Window, _: &mut Context<Self>) -> Option<Range<usize>> {
        self.marked.as_ref().map(|range| self.range_to_utf16(range))
    }

    fn unmark_text(&mut self, _: &mut Window, _: &mut Context<Self>) {
        self.marked = None;
    }

    fn replace_text_in_range(
        &mut self,
        range_utf16: Option<Range<usize>>,
        new_text: &str,
        _: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let range = range_utf16
            .as_ref()
            .map(|range| self.range_from_utf16(range))
            .or(self.marked.clone())
            .unwrap_or(self.selected.clone());
        let new_text = new_text.replace("\r\n", "\n");
        let kind = if new_text.chars().count() == 1 && !new_text.contains('\n') {
            EditKind::Typing
        } else {
            EditKind::Other
        };
        self.edit(range, &new_text, kind, cx);
    }

    fn replace_and_mark_text_in_range(
        &mut self,
        range_utf16: Option<Range<usize>>,
        new_text: &str,
        new_selected_range_utf16: Option<Range<usize>>,
        _: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let range = range_utf16
            .as_ref()
            .map(|range| self.range_from_utf16(range))
            .or(self.marked.clone())
            .unwrap_or(self.selected.clone());
        self.edit(range.clone(), new_text, EditKind::Typing, cx);
        self.marked = (!new_text.is_empty()).then(|| range.start..range.start + new_text.len());
        if let Some(selected) = new_selected_range_utf16 {
            // Relative au texte marqué, en UTF-16.
            let base = self.offset_to_utf16(range.start);
            let start = self.offset_from_utf16(base + selected.start);
            let end = self.offset_from_utf16(base + selected.end);
            self.select_range(start..end, false);
        }
        cx.notify();
    }

    fn bounds_for_range(
        &mut self,
        range_utf16: Range<usize>,
        _: Bounds<Pixels>,
        _: &mut Window,
        _: &mut Context<Self>,
    ) -> Option<Bounds<Pixels>> {
        let layout = self.layout.as_ref()?;
        let range = self.range_from_utf16(&range_utf16);
        let (row, col) = self.buffer.point(range.start);
        let column = display_column(self.buffer.line(row), col);
        let x = layout.text_left - self.scroll.x + layout.char_width * column as f32;
        let y = layout.bounds.top() + layout.line_height * row as f32 - self.scroll.y;
        Some(Bounds::new(
            point(x, y),
            size(layout.char_width, layout.line_height),
        ))
    }

    fn character_index_for_point(
        &mut self,
        point: Point<Pixels>,
        _: &mut Window,
        _: &mut Context<Self>,
    ) -> Option<usize> {
        let offset = self.offset_for_position(point)?;
        Some(self.offset_to_utf16(offset))
    }
}

impl Focusable for CodeArea {
    fn focus_handle(&self, _: &App) -> FocusHandle {
        self.focus_handle.clone()
    }
}

impl Render for CodeArea {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = Theme::light();
        div()
            .size_full()
            .flex()
            .flex_col()
            .overflow_hidden()
            .key_context(KEY_CONTEXT)
            .track_focus(&self.focus_handle)
            .cursor(CursorStyle::IBeam)
            .bg(theme.code_bg)
            .font_family(fonts::MONO)
            .text_size(metrics::TEXT_BASE)
            .on_action(cx.listener(Self::backspace))
            .on_action(cx.listener(Self::delete_word_left))
            .on_action(cx.listener(Self::delete_to_line_start))
            .on_action(cx.listener(Self::delete))
            .on_action(cx.listener(Self::left))
            .on_action(cx.listener(Self::right))
            .on_action(cx.listener(Self::up))
            .on_action(cx.listener(Self::down))
            .on_action(cx.listener(Self::select_left))
            .on_action(cx.listener(Self::select_right))
            .on_action(cx.listener(Self::select_up))
            .on_action(cx.listener(Self::select_down))
            .on_action(cx.listener(Self::word_left_action))
            .on_action(cx.listener(Self::word_right_action))
            .on_action(cx.listener(Self::select_word_left))
            .on_action(cx.listener(Self::select_word_right))
            .on_action(cx.listener(Self::line_start))
            .on_action(cx.listener(Self::line_end))
            .on_action(cx.listener(Self::select_line_start))
            .on_action(cx.listener(Self::select_line_end))
            .on_action(cx.listener(Self::doc_start))
            .on_action(cx.listener(Self::doc_end))
            .on_action(cx.listener(Self::select_doc_start))
            .on_action(cx.listener(Self::select_doc_end))
            .on_action(cx.listener(Self::page_up))
            .on_action(cx.listener(Self::page_down))
            .on_action(cx.listener(Self::select_all))
            .on_action(cx.listener(Self::newline))
            .on_action(cx.listener(Self::indent_action))
            .on_action(cx.listener(Self::outdent))
            .on_action(cx.listener(Self::toggle_comment))
            .on_action(cx.listener(Self::delete_line))
            .on_action(cx.listener(Self::copy))
            .on_action(cx.listener(Self::cut))
            .on_action(cx.listener(Self::paste))
            .on_action(cx.listener(Self::undo_action))
            .on_action(cx.listener(Self::redo_action))
            .on_action(cx.listener(Self::save))
            .on_action(cx.listener(Self::cancel))
            .on_action(cx.listener(Self::show_character_palette))
            .on_mouse_down(MouseButton::Left, cx.listener(Self::on_mouse_down))
            .on_mouse_up(MouseButton::Left, cx.listener(Self::on_mouse_up))
            .on_mouse_up_out(MouseButton::Left, cx.listener(Self::on_mouse_up))
            .on_mouse_move(cx.listener(Self::on_mouse_move))
            .on_scroll_wheel(cx.listener(Self::on_scroll))
            .child(div().flex_1().min_h_0().size_full().child(CodeElement {
                area: cx.entity(),
                theme,
            }))
            .child(self.render_position(theme))
    }
}

impl CodeArea {
    /// Ligne d'état de l'éditeur (position, sélection, problème de la ligne) :
    /// dessinée ici pour qu'un déplacement du curseur ne redessine que l'éditeur.
    fn render_position(&self, theme: Theme) -> impl IntoElement {
        let (line, column) = self.cursor_position();
        let mut position = format!("Ligne {line}, colonne {column}");
        let selected = self.selection_len();
        if selected > 0 {
            position.push_str(&format!(" · {selected} sélectionnés"));
        }
        let here = self
            .diagnostics
            .iter()
            .filter(|d| d.row + 1 == line)
            .min_by_key(|d| severity_rank(d.severity))
            .cloned();
        div()
            .flex_none()
            .flex()
            .items_center()
            .gap(metrics::SPACE_3)
            .h(metrics::CONTROL_H)
            .px(metrics::SPACE_3)
            .bg(theme.bg_surface)
            .border_t_1()
            .border_color(theme.border_soft)
            .font_family(fonts::SANS)
            .text_size(metrics::TEXT_SM)
            .text_color(theme.ink_muted)
            .child(div().flex_none().font_family(fonts::MONO).child(position))
            .when(self.is_dirty(), |this| {
                this.child(div().flex_none().child("Modifié"))
            })
            .children(here.map(|diagnostic| {
                let color = match diagnostic.severity {
                    Severity::Critical => theme.danger,
                    Severity::Warning => theme.warning,
                    Severity::Info => theme.ink_soft,
                };
                div()
                    .min_w_0()
                    .truncate()
                    .text_color(color)
                    .child(diagnostic.message)
            }))
    }
}

// ---------------------------------------------------------------------------
// Dessin

struct CodeElement {
    area: Entity<CodeArea>,
    theme: Theme,
}

struct PaintedLine {
    row: usize,
    y: Pixels,
    text: ShapedLine,
    number: ShapedLine,
    background: Option<Hsla>,
    marker: Option<Hsla>,
}

struct PrepaintState {
    layout: Layout,
    gutter_right: Pixels,
    lines: Vec<PaintedLine>,
    selections: Vec<PaintQuad>,
    cursor: Option<PaintQuad>,
}

impl IntoElement for CodeElement {
    type Element = Self;

    fn into_element(self) -> Self::Element {
        self
    }
}

fn severity_rank(severity: Severity) -> u8 {
    match severity {
        Severity::Critical => 0,
        Severity::Warning => 1,
        Severity::Info => 2,
    }
}

impl Element for CodeElement {
    type RequestLayoutState = ();
    type PrepaintState = PrepaintState;

    fn id(&self) -> Option<ElementId> {
        None
    }

    fn source_location(&self) -> Option<&'static core::panic::Location<'static>> {
        None
    }

    fn request_layout(
        &mut self,
        _: Option<&GlobalElementId>,
        _: Option<&gpui::InspectorElementId>,
        window: &mut Window,
        cx: &mut App,
    ) -> (LayoutId, Self::RequestLayoutState) {
        let mut style = Style::default();
        style.size.width = relative(1.).into();
        style.size.height = relative(1.).into();
        (window.request_layout(style, [], cx), ())
    }

    fn prepaint(
        &mut self,
        _: Option<&GlobalElementId>,
        _: Option<&gpui::InspectorElementId>,
        bounds: Bounds<Pixels>,
        _: &mut Self::RequestLayoutState,
        window: &mut Window,
        cx: &mut App,
    ) -> Self::PrepaintState {
        let theme = self.theme;
        let font_size = metrics::TEXT_BASE;
        let line_height = metrics::SPACE_5;
        let mono: Font = font(fonts::MONO);
        let font_id = window.text_system().resolve_font(&mono);
        let char_width = window
            .text_system()
            .advance(font_id, font_size, 'm')
            .map_or(FALLBACK_CHAR_WIDTH, |advance| advance.width);

        let focused = self.area.read(cx).focus_handle.is_focused(window);
        let line_count = self.area.read(cx).buffer.line_count();
        let digits = line_count.to_string().len().max(3);
        let gutter_right =
            bounds.left() + MARKER_COLUMN + char_width * digits as f32 + metrics::SPACE_2;
        let text_left = gutter_right + metrics::SPACE_3;
        let text_width = bounds.right() - text_left;

        // Défilement automatique vers le curseur.
        self.area.update(cx, |area, _| {
            let Some(mode) = area.autoscroll.take() else {
                return;
            };
            let (row, col) = area.buffer.point(area.cursor());
            let top = line_height * row as f32;
            let height = bounds.size.height;
            match mode {
                Autoscroll::Center => {
                    area.scroll.y = (top - height / 2.).max(ZERO);
                }
                Autoscroll::Fit => {
                    let margin = line_height * 2.;
                    if top - margin < area.scroll.y {
                        area.scroll.y = (top - margin).max(ZERO);
                    } else if top + line_height + margin > area.scroll.y + height {
                        area.scroll.y = top + line_height + margin - height;
                    }
                }
            }
            let x = char_width * display_column(area.buffer.line(row), col) as f32;
            let margin = char_width * 4.;
            if x - margin < area.scroll.x {
                area.scroll.x = (x - margin).max(ZERO);
            } else if x + margin > area.scroll.x + text_width {
                area.scroll.x = x + margin - text_width;
            }
        });

        let area = self.area.read(cx);
        let scroll = area.scroll;
        let first_row = ((scroll.y / line_height).floor().max(0.) as usize).min(line_count - 1);
        let visible = (bounds.size.height / line_height).ceil() as usize + 1;
        let last_row = (first_row + visible).min(line_count);

        // Gravité la plus forte par ligne.
        let mut worst: std::collections::HashMap<usize, Severity> =
            std::collections::HashMap::new();
        for diagnostic in &area.diagnostics {
            let entry = worst.entry(diagnostic.row).or_insert(diagnostic.severity);
            if severity_rank(diagnostic.severity) < severity_rank(*entry) {
                *entry = diagnostic.severity;
            }
        }

        let cursor = area.cursor();
        let (cursor_row, _) = area.buffer.point(cursor);
        let base = TextRun {
            len: 0,
            font: mono.clone(),
            color: theme.code_text,
            background_color: None,
            underline: None,
            strikethrough: None,
        };

        let mut lines = Vec::with_capacity(last_row - first_row);
        let mut selections = Vec::new();
        let mut cursor_quad = None;
        for row in first_row..last_row {
            let y = bounds.top() + line_height * row as f32 - scroll.y;
            let raw = area.buffer.line(row);
            let shown = display_line(raw);

            // Couleurs : coloration, puis soulignement du texte en cours de composition.
            let spans = area.syntax.highlight(&shown, &theme);
            let mut colors: Vec<(Range<usize>, Hsla)> = Vec::new();
            let mut at = 0;
            for (range, color) in spans {
                if range.start > at {
                    colors.push((at..range.start, theme.code_text));
                }
                colors.push((range.clone(), color));
                at = range.end;
            }
            if at < shown.len() {
                colors.push((at..shown.len(), theme.code_text));
            }
            let marked = area.marked.as_ref().and_then(|marked| {
                let range = area.buffer.line_range(row);
                (marked.start >= range.start && marked.end <= range.end).then(|| {
                    display_offset(raw, marked.start - range.start)
                        ..display_offset(raw, marked.end - range.start)
                })
            });
            let runs: Vec<TextRun> = colors
                .into_iter()
                .flat_map(|(range, color)| {
                    // Découpe autour du texte marqué pour le souligner.
                    let mut parts = vec![(range.clone(), false)];
                    if let Some(m) = &marked {
                        parts = [
                            (range.start..range.end.min(m.start), false),
                            (range.start.max(m.start)..range.end.min(m.end), true),
                            (range.start.max(m.end)..range.end, false),
                        ]
                        .into_iter()
                        .filter(|(r, _)| r.start < r.end)
                        .collect();
                    }
                    parts
                        .into_iter()
                        .map(move |(r, underline)| (r, color, underline))
                })
                .map(|(range, color, underline)| TextRun {
                    len: range.len(),
                    color,
                    underline: underline.then_some(UnderlineStyle {
                        color: Some(color),
                        thickness: UNDERLINE_THICKNESS,
                        wavy: false,
                    }),
                    ..base.clone()
                })
                .collect();
            let text =
                window
                    .text_system()
                    .shape_line(shown.clone().into(), font_size, &runs, None);

            let number_color = if row == cursor_row {
                theme.code_text
            } else {
                theme.code_line_number
            };
            let number_text: SharedString = (row + 1).to_string().into();
            let number = window.text_system().shape_line(
                number_text.clone(),
                font_size,
                &[TextRun {
                    len: number_text.len(),
                    color: number_color,
                    ..base.clone()
                }],
                None,
            );

            let severity = worst.get(&row).copied();
            let background = match severity {
                Some(Severity::Critical) => Some(theme.code_line_critical),
                Some(Severity::Warning) => Some(theme.code_line_warning),
                _ if row == cursor_row && area.selected.is_empty() && focused => {
                    Some(theme.code_current_line)
                }
                _ => None,
            };
            let marker = severity.map(|severity| match severity {
                Severity::Critical => theme.danger,
                Severity::Warning => theme.warning,
                Severity::Info => theme.ink_soft,
            });

            // Sélection sur cette ligne.
            let range = area.buffer.line_range(row);
            if !area.selected.is_empty()
                && area.selected.start <= range.end
                && area.selected.end >= range.start
            {
                let start = area.selected.start.max(range.start) - range.start;
                let end = area.selected.end.min(range.end) - range.start;
                let x0 = text.x_for_index(display_offset(raw, start));
                let mut x1 = text.x_for_index(display_offset(raw, end));
                // La sélection continue après la fin de ligne : montrer le saut de ligne.
                if area.selected.end > range.end {
                    x1 += char_width;
                }
                if x1 > x0 {
                    selections.push(fill(
                        Bounds::from_corners(
                            point(text_left - scroll.x + x0, y),
                            point(text_left - scroll.x + x1, y + line_height),
                        ),
                        theme.code_selection,
                    ));
                }
            }

            if row == cursor_row && focused {
                let col = cursor - range.start;
                let x = text.x_for_index(display_offset(raw, col));
                cursor_quad = Some(fill(
                    Bounds::new(
                        point(text_left - scroll.x + x, y),
                        size(CURSOR_WIDTH, line_height),
                    ),
                    theme.ink,
                ));
            }

            lines.push(PaintedLine {
                row,
                y,
                text,
                number,
                background,
                marker,
            });
        }

        let layout = Layout {
            bounds,
            text_left,
            line_height,
            char_width,
            lines: lines
                .iter()
                .map(|line| (line.row, line.text.clone()))
                .collect(),
        };
        PrepaintState {
            layout,
            gutter_right,
            lines,
            selections,
            cursor: cursor_quad,
        }
    }

    fn paint(
        &mut self,
        _: Option<&GlobalElementId>,
        _: Option<&gpui::InspectorElementId>,
        bounds: Bounds<Pixels>,
        _: &mut Self::RequestLayoutState,
        prepaint: &mut Self::PrepaintState,
        window: &mut Window,
        cx: &mut App,
    ) {
        let theme = self.theme;
        let focus_handle = self.area.read(cx).focus_handle.clone();
        window.handle_input(
            &focus_handle,
            ElementInputHandler::new(bounds, self.area.clone()),
            cx,
        );
        let line_height = prepaint.layout.line_height;
        let text_left = prepaint.layout.text_left;
        let scroll_x = self.area.read(cx).scroll.x;

        window.with_content_mask(Some(ContentMask { bounds }), |window| {
            // Fonds de ligne (problème, ligne courante) sur toute la largeur.
            for line in &prepaint.lines {
                if let Some(color) = line.background {
                    window.paint_quad(fill(
                        Bounds::new(
                            point(bounds.left(), line.y),
                            size(bounds.size.width, line_height),
                        ),
                        color,
                    ));
                }
            }

            // Texte, sélection et curseur, coupés au bord de la marge.
            let text_bounds = Bounds::from_corners(
                point(text_left - metrics::SPACE_1, bounds.top()),
                bounds.bottom_right(),
            );
            window.with_content_mask(
                Some(ContentMask {
                    bounds: text_bounds,
                }),
                |window| {
                    for quad in prepaint.selections.drain(..) {
                        window.paint_quad(quad);
                    }
                    for line in &prepaint.lines {
                        line.text
                            .paint(point(text_left - scroll_x, line.y), line_height, window, cx)
                            .ok();
                    }
                    if let Some(cursor) = prepaint.cursor.take() {
                        window.paint_quad(cursor);
                    }
                },
            );

            // Marge : numéros alignés à droite et marqueurs de problème.
            window.paint_quad(fill(
                Bounds::from_corners(
                    point(prepaint.gutter_right + metrics::SPACE_2, bounds.top()),
                    point(
                        prepaint.gutter_right + metrics::SPACE_2 + RULE_WIDTH,
                        bounds.bottom(),
                    ),
                ),
                theme.border_soft,
            ));
            for line in &prepaint.lines {
                let x = prepaint.gutter_right - line.number.width;
                line.number
                    .paint(point(x, line.y), line_height, window, cx)
                    .ok();
                if let Some(color) = line.marker {
                    let offset = (line_height - MARKER_SIZE) / 2.;
                    window.paint_quad(
                        fill(
                            Bounds::new(
                                point(
                                    bounds.left() + (MARKER_COLUMN - MARKER_SIZE) / 2.,
                                    line.y + offset,
                                ),
                                size(MARKER_SIZE, MARKER_SIZE),
                            ),
                            color,
                        )
                        .corner_radii(MARKER_SIZE / 2.),
                    );
                }
            }
        });

        let layout = prepaint.layout.clone();
        self.area.update(cx, |area, _| area.layout = Some(layout));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn buffer_indexes_lines() {
        let buffer = Buffer::new("ab\n\tc\n".to_owned());
        assert_eq!(buffer.line_count(), 3);
        assert_eq!(buffer.line(1), "\tc");
        assert_eq!(buffer.line(2), "");
        assert_eq!(buffer.point(4), (1, 1));
        assert_eq!(buffer.widest, TAB_WIDTH + 1);
    }

    /// L'index mis à jour sur place est identique à un index recalculé, pour
    /// toutes les plages et tous les remplacements de quelques textes.
    #[test]
    fn incremental_line_index_matches_full_reindex() {
        let texts = ["", "a", "ab\ncd\n", "\n\n", "x\ny\nz", "é\n\tû\n"];
        let inserts = ["", "k", "\n", "a\nb", "\n\n", "é\n"];
        for text in texts {
            let bounds: Vec<usize> = (0..=text.len())
                .filter(|&i| text.is_char_boundary(i))
                .collect();
            for &start in &bounds {
                for &end in bounds.iter().filter(|&&end| end >= start) {
                    for insert in inserts {
                        let mut buffer = Buffer::new(text.to_owned());
                        buffer.replace(start..end, insert);
                        let expected = Buffer::new(buffer.text.clone());
                        assert_eq!(
                            buffer.line_starts, expected.line_starts,
                            "{text:?} [{start}..{end}] ← {insert:?}"
                        );
                    }
                }
            }
        }
    }

    #[test]
    fn maps_tabs_between_content_and_display() {
        let line = "\tx";
        assert_eq!(display_offset(line, 1), TAB_WIDTH);
        assert_eq!(content_offset(line, TAB_WIDTH), 1);
        assert_eq!(content_offset(line, 1), 0);
        assert_eq!(offset_for_column(line, TAB_WIDTH + 1), 2);
    }

    #[test]
    fn detects_indent_unit() {
        assert_eq!(detect_indent("a\n\tb\n\tc\n"), "\t");
        assert_eq!(detect_indent("a\n  b\n    c\n"), "  ");
        assert_eq!(detect_indent("a\n    b\n"), "    ");
    }

    #[test]
    fn highlights_comments_strings_and_keywords() {
        let theme = Theme::light();
        let syntax = Syntax::for_path("src/main.rs");
        let spans = syntax.highlight("let s = \"a\"; // note", &theme);
        assert!(
            spans
                .iter()
                .any(|(r, c)| *r == (0..3) && *c == theme.code_keyword)
        );
        assert!(
            spans
                .iter()
                .any(|(r, c)| *r == (8..11) && *c == theme.code_string)
        );
        assert!(spans.iter().any(|(_, c)| *c == theme.code_comment));
        assert_eq!(Syntax::for_path("app.py").line_comment, "#");
    }
}
