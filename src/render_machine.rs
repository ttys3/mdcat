// Copyright 2019 Sebastian Wiesner <sebastian@swsnr.de>

// Licensed under the Apache License, Version 2.0 (the "License");
// you may not use this file except in compliance with the License.
// You may obtain a copy of the License at

//  http://www.apache.org/licenses/LICENSE-2.0

// Unless required by applicable law or agreed to in writing, software
// distributed under the License is distributed on an "AS IS" BASIS,
// WITHOUT WARRANTIES OR CONDITIONS OF ANY KIND, either express or implied.
// See the License for the specific language governing permissions and
// limitations under the License.

use crate::terminal::StyleCapability;
use crate::terminal::TerminalCapabilities;
use ansi_term::{Colour, Style};
use pulldown_cmark::{CowStr, Event};
use std::io::prelude::*;
use std::io::Result;
use syntect::highlighting::{HighlightState, Highlighter, Theme};
use syntect::parsing::{ParseState, ScopeStack, SyntaxReference, SyntaxSet};

/// Style of inline text.
#[derive(Default, PartialEq, Debug)]
struct InlineStyle {
    /// The level of emphasis we're currently in.
    emphasis_level: usize,
    /// The current style or none if plain text.
    style: Option<Style>,
    /// Parent styles of this style.
    parent_styles: Vec<Style>,
}

impl InlineStyle {
    /// Push the given style.
    fn push_style(mut self, style: Style) -> Self {
        if let Some(current) = self.style {
            self.parent_styles.push(current);
        }
        self.style = Some(style);
        self
    }

    /// Push a new style as a modification of the current style.
    fn push_changed_style<F>(self, f: F) -> Self
    where
        F: FnOnce(&Style) -> Style,
    {
        let style = self.style.unwrap_or_else(|| Style::new());
        self.push_style(f(&style))
    }

    /// Pop the last style.
    fn pop_style(mut self) -> Self {
        self.style = self.parent_styles.pop();
        self
    }

    /// Remove one level of emphasis.
    fn remove_emphasis(mut self) -> Self {
        self.emphasis_level -= 1;
        self.toggle_italics_for_emphasis()
    }

    /// Add one level of emphasis.
    fn add_emphasis(mut self) -> Self {
        self.emphasis_level += 1;
        self.toggle_italics_for_emphasis()
    }

    /// Toggle emhpasis according to the current emphasis level.
    fn toggle_italics_for_emphasis(self) -> Self {
        let is_italic = self.emphasis_level % 2 == 1;
        self.push_changed_style(|&s| Style { is_italic, ..s })
    }
}

/// State of the rendering state machine.
#[derive(Debug)]
enum RenderState<'a> {
    /// The initial state, before anything is printed at all.
    ///
    /// Used to suppress leading line breaks.
    Initial,
    /// Top-level state, waiting for the next block level element.
    TopLevel,
    /// Styled inline text.
    StyledInline(InlineStyle),
    /// A raw code block without any syntax highlighting.
    RawCodeBlock,
    /// A highlighted code block.
    HighlightedCodeBlock(&'a SyntaxReference, ParseState, HighlightState),
    Error,
}

/// Start a header.
///
/// Write a header adornment for a header of the given `level` to the given `writer`, using styling
/// `capability` if any.
fn start_header<'a, W: Write>(
    writer: &mut W,
    level: usize,
    capability: &StyleCapability,
) -> Result<RenderState<'a>> {
    use crate::terminal::StyleCapability::Ansi;
    let adornment = "\u{2504}".repeat(level);
    let style = Style::new().fg(Colour::Blue).bold();
    if let Ansi(ansi) = capability {
        ansi.write_styled(writer, &style, adornment)?;
    } else {
        write!(writer, "{}", adornment)?;
    }
    Ok(RenderState::StyledInline(
        InlineStyle::default().push_style(style),
    ))
}

fn write_border<W: Write>(writer: &mut W, capability: &StyleCapability, size: usize) -> Result<()> {
    use crate::terminal::StyleCapability::Ansi;
    let separator = "\u{2500}".repeat(size);
    if let Ansi(ansi) = capability {
        let style = Style::new().fg(Colour::Green);
        ansi.write_styled(writer, &style, separator)?;
        writeln!(writer)
    } else {
        writeln!(writer, "{}", separator)
    }
}

fn start_codeblock<'a, W: Write>(
    writer: &mut W,
    language: CowStr,
    style: &StyleCapability,
    syntax_set: &'a SyntaxSet,
    theme: &'a Theme,
) -> Result<RenderState<'a>> {
    write_border(writer, style, 20)?;
    let syntax = if language.is_empty() {
        None
    } else {
        syntax_set.find_syntax_by_token(&language)
    };
    Ok(syntax
        .map(|syntax| {
            let hstate = HighlightState::new(&Highlighter::new(theme), ScopeStack::new());
            let pstate = ParseState::new(syntax);
            RenderState::HighlightedCodeBlock(syntax, pstate, hstate)
        })
        .unwrap_or_else(|| RenderState::RawCodeBlock))
}

fn end_codeblock<'a, W: Write>(writer: &mut W, style: &StyleCapability) -> Result<RenderState<'a>> {
    write_border(writer, style, 20)?;
    Ok(RenderState::TopLevel)
}

/// Proess a single `event`.
///
/// Render the representation of `event` to the given `writer`, in the current `state`, using the
/// given terminal `capabilities` for rendering. `syntax_set` provides language grammars to
/// highlight code in code blocks.
///
/// Return the next rendering state.
fn process_event<'a, W: Write>(
    writer: &mut W,
    state: RenderState<'a>,
    event: Event<'a>,
    capabilities: &TerminalCapabilities,
    syntax_set: &'a SyntaxSet,
    theme: &'a Theme,
) -> Result<RenderState<'a>> {
    use crate::terminal::StyleCapability::*;
    use pulldown_cmark::Event::*;
    use pulldown_cmark::Tag::*;
    use RenderState::*;
    // THE BIG DISPATCH
    match (state, event) {
        // Enter a header
        (Initial, Start(Header(level))) => {
            start_header(writer, level as usize, &capabilities.style)
        }
        (TopLevel, Start(Header(level))) => {
            // Add a margin before the last block
            writeln!(writer)?;
            start_header(writer, level as usize, &capabilities.style)
        }
        // Enter a paragraph, either top-level or initial
        (Initial, Start(Paragraph)) => Ok(StyledInline(InlineStyle::default())),
        (TopLevel, Start(Paragraph)) => {
            // Add a margin before the last block
            writeln!(writer)?;
            Ok(StyledInline(InlineStyle::default()))
        }
        // Inline markup in line text starts
        (StyledInline(inline), Start(Strong)) => {
            Ok(StyledInline(inline.push_changed_style(|s| s.bold())))
        }
        (StyledInline(inline), Start(Strikethrough)) => Ok(StyledInline(
            inline.push_changed_style(|s| s.strikethrough()),
        )),
        (StyledInline(inline), Start(Code)) => Ok(StyledInline(
            inline.push_changed_style(|s| s.fg(Colour::Yellow)),
        )),
        (StyledInline(inline), Start(Emphasis)) => Ok(StyledInline(inline.add_emphasis())),
        // …and ends
        (StyledInline(inline), End(Strong))
        | (StyledInline(inline), End(Strikethrough))
        | (StyledInline(inline), End(Code)) => Ok(StyledInline(inline.pop_style())),
        (StyledInline(inline), End(Emphasis)) => Ok(StyledInline(inline.remove_emphasis())),
        // Inline text with styling
        (StyledInline(styles), Text(s)) => {
            if let Ansi(ansi) = &capabilities.style {
                let style = styles.style.unwrap_or_else(|| Style::new());
                ansi.write_styled(writer, &style, s)?;
            } else {
                write!(writer, "{}", s)?;
            }
            Ok(StyledInline(styles))
        }
        // Line breaks in inline text
        (s @ StyledInline(_), SoftBreak) | (s @ StyledInline(_), HardBreak) => {
            writeln!(writer)?;
            Ok(s)
        }
        // Inline ends
        (StyledInline(_), End(Paragraph)) => {
            writeln!(writer)?;
            Ok(RenderState::TopLevel)
        }
        (StyledInline(_), End(Header(_))) => {
            writeln!(writer)?;
            Ok(RenderState::TopLevel)
        }
        // Code blocks, either raw or with syntax highlighting
        (Initial, Start(CodeBlock(language))) => {
            start_codeblock(writer, language, &capabilities.style, syntax_set, theme)
        }
        (TopLevel, Start(CodeBlock(language))) => {
            writeln!(writer)?;
            start_codeblock(writer, language, &capabilities.style, syntax_set, theme)
        }
        (RawCodeBlock, Text(s)) => {
            use crate::terminal::StyleCapability::Ansi;
            if let Ansi(ansi) = &capabilities.style {
                ansi.write_styled(writer, &Style::new().fg(Colour::Yellow), s)?;
            } else {
                write!(writer, "{}", s)?;
            }
            Ok(RawCodeBlock)
        }
        (HighlightedCodeBlock(..), Text(s)) => unimplemented!(),
        (RawCodeBlock, End(CodeBlock(_))) => end_codeblock(writer, &capabilities.style),
        (HighlightedCodeBlock(..), End(CodeBlock(_))) => end_codeblock(writer, &capabilities.style),
        _ => Ok(Error),
    }
}

/// Render Markdown to TTY.
///
/// Render markdown `events` to a `writer`.
///
/// `capabilities` denotes what the terminal emulator behind the `writer` can do wrt to styling and
/// other features.  `syntax_set` provides language grammars for highlighting code blocks.
pub fn render<'a, I, W>(
    writer: &mut W,
    events: I,
    capabilities: &TerminalCapabilities,
    syntax_set: &SyntaxSet,
    theme: &Theme,
) -> Result<()>
where
    W: Write,
    I: Iterator<Item = Event<'a>>,
{
    let mut state = RenderState::Initial;
    for event in events {
        let error_msg = format!("{:?} {:?}", &state, &event);
        let next_state = process_event(writer, state, event, capabilities, syntax_set, theme)?;
        match next_state {
            RenderState::Error => panic!("Rendering errored: {}", error_msg),
            _ => state = next_state,
        }
    }
    Ok(())
}
