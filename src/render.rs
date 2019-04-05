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

use ansi_term::{Colour, Style};
use pulldown_cmark::Event::*;
use pulldown_cmark::Tag::*;
use pulldown_cmark::{CowStr, Event};

/// An event for printing to TTY.
#[derive(Debug)]
pub enum PrintEvent<'a> {
    /// A text with some style.
    StyledText(CowStr<'a>, Style),
    /// A newline
    Newline,
    /// A margin, that is, an empty line, at the end of block elements
    Margin,
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

/// Inject margins into a stream of events
fn inject_margins<'a, I>(events: I) -> impl Iterator<Item = PassEvent<'a>>
where
    I: Iterator<Item = PassEvent<'a>>,
{
    use PrintEvent::Margin;
    events.flat_map(|e| match e {
        Markdown(End(Paragraph)) => vec![e, Print(Margin)],
        Markdown(End(BlockQuote)) => vec![e, Print(Margin)],
        Markdown(End(List(_))) => vec![e, Print(Margin)],
        Markdown(End(Header(_))) => vec![e, Print(Margin)],
        Markdown(End(CodeBlock(_))) => vec![e, Print(Margin)],
        Markdown(End(Rule)) => vec![e, Print(Margin)],
        _ => vec![e],
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
            Header(_) | Paragraph | Strikethrough | Strong | Emphasis | Code => false,
            _ => true,
        },
        _ => true,
    })
}

/// Render Markdown events into printing events.
///
/// Combines all passes in proper order.
pub struct Renderer<'a> {
    passes: Box<Iterator<Item = PassEvent<'a>> + 'a>,
}

impl<'a> Renderer<'a> {
    /// Create a renderer for the given markdown events.
    pub fn new<I>(events: I) -> Renderer<'a>
    where
        I: Iterator<Item = Event<'a>> + 'a,
    {
        let passes = remove_processed_markdown(break_lines(decorate_headers(style_text(
            inject_margins(events.map(PassEvent::Markdown)),
        ))));
        Renderer {
            passes: Box::new(passes),
        }
    }

    /// Iterate over the raw passes.
    pub fn raw(self) -> impl Iterator<Item = PassEvent<'a>> {
        self.passes
    }
}

impl<'a> Iterator for Renderer<'a> {
    type Item = PrintEvent<'a>;

    fn next(&mut self) -> Option<Self::Item> {
        self.passes.next().map(|e| match e {
            Print(print_event) => print_event,
            Markdown(markdown_event) => panic!(
                "Unexpected markdown event after rendering: {:?}",
                markdown_event
            ),
        })
    }
}
