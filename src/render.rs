// Copyright 2019 Sebastian Wiesner <sebastian@swsnr.de>

// Licensed under the Apache License, Version 2.0 (the "License");
// you may not use this file except in compliance with the License.
// You may obtain a copy of the License at

// 	http://www.apache.org/licenses/LICENSE-2.0

// Unless required by applicable law or agreed to in writing, software
// distributed under the License is distributed on an "AS IS" BASIS,
// WITHOUT WARRANTIES OR CONDITIONS OF ANY KIND, either express or implied.
// See the License for the specific language governing permissions and
// limitations under the License.

//! Render markdown to TTY.
//!
//! This module and its submodule process a stream of Markdown events into a stream of what we call
//! "print events" which turn Markdown into a line-oriented document for printing, and ultimately
//! into a series of "style strings" which we can finally print to a TTY.
//!
//! Rendering happens in multiple passes, each of which turns a certain kind of Markdown events
//! into print events.  Each pass runs as a lazy iterator; while we sometimes do need to drag state
//! along the events we try to retain the streaming interface of pulldown cmark.

use super::highlighting;
use ansi_term::{Colour, Style};
use pulldown_cmark::Event::*;
use pulldown_cmark::Tag::*;
use pulldown_cmark::{CowStr, Event};
use syntect::easy::HighlightLines;
use syntect::highlighting::Theme;
use syntect::parsing::SyntaxSet;

/// A ruler.
#[derive(Debug, Copy, Clone, PartialEq)]
pub struct Ruler {
    /// The character to draw the ruler with.
    pub character: char,
    /// The style of the ruler.
    pub style: Style,
    /// The max width.
    ///
    /// If none the ruler spans the entire width of the terminal.  If `width` exceeds the width
    /// of the terminal the ruler cuts off at the end of the line.
    pub width: Option<usize>,
}

/// An event for printing to TTY.
#[derive(Debug, Clone, PartialEq)]
pub enum PrintEvent<'a> {
    /// A text with some style.
    StyledText(CowStr<'a>, Style),
    /// A ruler.
    Ruler(Ruler),
    /// A newline
    Newline,
}

/// An event resulting from a pass.
///
/// Either a raw Markdown event, or a print event.
#[derive(Debug)]
pub enum PassEvent<'a> {
    /// A raw markdown event.
    ///
    /// Normally something a pass didn't touch.
    Markdown(Event<'a>),
    /// A event for printing to a TTY.
    Print(PrintEvent<'a>),
}

use PassEvent::*;

/// Inject margins at the start of certain blocks.
fn inject_margins<'a, I>(events: I) -> impl Iterator<Item = PassEvent<'a>>
where
    I: Iterator<Item = PassEvent<'a>>,
{
    use PrintEvent::Newline;
    events
        .flat_map(|e| match e {
            Markdown(Start(Paragraph)) => vec![Print(Newline), e],
            Markdown(Start(BlockQuote)) => vec![Print(Newline), e],
            Markdown(Start(List(_))) => vec![Print(Newline), e],
            Markdown(Start(Header(_))) => vec![Print(Newline), e],
            Markdown(Start(CodeBlock(_))) => vec![Print(Newline), e],
            Markdown(Start(Rule)) => vec![Print(Newline), e],
            _ => vec![e],
        })
        .skip_while(|e| match e {
            Print(Newline) => true,
            _ => false,
        })
}

/// Add decorations to headers.
fn decorate_headers<'a, I>(events: I) -> impl Iterator<Item = PassEvent<'a>>
where
    I: Iterator<Item = PassEvent<'a>>,
{
    use PrintEvent::StyledText;
    events.flat_map(|e| match e {
        Markdown(Start(Header(level))) => vec![
            Print(StyledText(
                CowStr::Boxed("\u{2504}".repeat(level as usize).into_boxed_str()),
                Style::new().fg(Colour::Blue).bold(),
            )),
            e,
        ],
        _ => vec![e],
    })
}

/// Highlight code in code blocks.
pub fn highlight_code<'a: 'c, 'b: 'c, 'c, I>(
    events: I,
    syntax_set: &'b SyntaxSet,
    theme: &'b Theme,
) -> impl Iterator<Item = PassEvent<'a>> + 'c
where
    I: Iterator<Item = PassEvent<'a>> + 'c,
{
    let mut current_highlighter = None;
    let ruler = Ruler {
        character: '\u{2500}',
        style: Style::new().fg(Colour::Green),
        width: Some(20),
    };

    events.flat_map(move |e| match e {
        Markdown(Start(CodeBlock(ref language))) => {
            if !language.is_empty() {
                current_highlighter = syntax_set
                    .find_syntax_by_token(&language)
                    .map(|syntax| HighlightLines::new(syntax, theme));
            }
            vec![Print(PrintEvent::Ruler(ruler)), e]
        }
        Markdown(End(CodeBlock(_))) => {
            current_highlighter = None;
            vec![e, Print(PrintEvent::Ruler(ruler))]
        }
        Markdown(Text(s)) => match &mut current_highlighter {
            None => vec![Markdown(Text(s))],
            Some(highlight) => {
                let regions = highlight.highlight(&s, &syntax_set);
                highlighting::to_ansi(&regions)
                    .map(|(style, s)| Print(PrintEvent::StyledText(CowStr::Boxed(s.into()), style)))
                    .collect()
            }
        },
        _ => vec![e],
    })
}

/// Style all text.
///
/// Adds styles to all text where styles are appropriate, by replacing Markdown Text events with
/// StyledText events.
pub fn style_text<'a, I>(events: I) -> impl Iterator<Item = PassEvent<'a>>
where
    I: Iterator<Item = PassEvent<'a>>,
{
    let mut previous_styles = Vec::new();
    let mut current_style = Style::new();
    let mut emphasis_level = 0;
    events.map(move |e| match e {
        Markdown(Start(ref tag)) => {
            match tag {
                Header(_) => {
                    previous_styles.push(current_style);
                    current_style = Style::new().fg(Colour::Blue).bold();
                }
                BlockQuote => {
                    emphasis_level += 1;
                    previous_styles.push(current_style);
                    current_style = Style {
                        is_italic: emphasis_level % 2 == 1,
                        ..current_style
                    }
                    .fg(Colour::Green);
                }
                Strikethrough => {
                    previous_styles.push(current_style);
                    current_style = current_style.strikethrough();
                }
                Strong => {
                    previous_styles.push(current_style);
                    current_style = current_style.bold();
                }
                Code | CodeBlock(_) => {
                    previous_styles.push(current_style);
                    current_style = current_style.fg(Colour::Yellow);
                }
                Emphasis => {
                    emphasis_level += 1;
                    previous_styles.push(current_style);
                    current_style = Style {
                        is_italic: emphasis_level % 2 == 1,
                        ..current_style
                    }
                }
                _ => (),
            };
            e
        }
        Markdown(End(ref tag)) => {
            match tag {
                CodeBlock(_) | Header(_) | Strikethrough | Strong | Code => {
                    current_style = previous_styles.pop().unwrap();
                }
                Emphasis | BlockQuote => {
                    emphasis_level -= 1;
                    current_style = previous_styles.pop().unwrap();
                }
                _ => (),
            };
            e
        }
        Markdown(Text(s)) => Print(PrintEvent::StyledText(s, current_style)),
        _ => e,
    })
}

/// Break lines.
///
/// Insert line breaks after block level elements, and replace hard and soft breaks with newlines.
pub fn break_lines<'a, I>(events: I) -> impl Iterator<Item = PassEvent<'a>>
where
    I: Iterator<Item = PassEvent<'a>>,
{
    // TODO: Insert line breaks at the end of list items
    events.flat_map(|e| match e {
        Markdown(End(Header(_))) => vec![e, Print(PrintEvent::Newline)],
        Markdown(End(Paragraph)) => vec![e, Print(PrintEvent::Newline)],
        Markdown(SoftBreak) | Markdown(HardBreak) => vec![Print(PrintEvent::Newline)],
        _ => vec![e],
    })
}

/// Erase inline markup text assuming inline tags were fully rendered.
pub fn remove_processed_markdown<'a, I>(events: I) -> impl Iterator<Item = PassEvent<'a>>
where
    I: Iterator<Item = PassEvent<'a>>,
{
    events.filter(|e| match e {
        Markdown(Start(t)) | Markdown(End(t)) => match t {
            CodeBlock(_) | Header(_) | Paragraph | Strikethrough | Strong | Emphasis | Code => {
                false
            }
            _ => true,
        },
        _ => true,
    })
}

// TODO: Return PrintEvent from `render` eventually.

/// Render markdown `events`.
///
/// Turn markdown events into print events for rendering to TTY.
///
/// `syntax_set` provides syntax definitions for highlighting code in code blocks.  `theme` is the
/// highlighting theme.
pub fn render<'a: 'c, 'b: 'c, 'c, I>(
    events: I,
    syntax_set: &'b SyntaxSet,
    theme: &'b Theme,
) -> Box<Iterator<Item = PassEvent<'a>> + 'c>
where
    I: Iterator<Item = Event<'a>> + 'c,
{
    let passes =
        remove_processed_markdown(break_lines(decorate_headers(style_text(highlight_code(
            inject_margins(events.map(PassEvent::Markdown)),
            syntax_set,
            theme,
        )))));
    Box::new(passes)
}
