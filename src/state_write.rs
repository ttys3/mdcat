// Copyright 2020 Sebastian Wiesner <sebastian@swsnr.de>

// Licensed under the Apache License, Version 2.0 (the "License");
// you may not use this file except in compliance with the License.
// You may obtain a copy of the License at

// 	http://www.apache.org/licenses/LICENSE-2.0

// Unless required by applicable law or agreed to in writing, software
// distributed under the License is distributed on an "AS IS" BASIS,
// WITHOUT WARRANTIES OR CONDITIONS OF ANY KIND, either express or implied.
// See the License for the specific language governing permissions and
// limitations under the License.

use crate::terminal::*;
use crate::Settings;
use ansi_term::{Colour, Style};
use pulldown_cmark::Event::*;
use pulldown_cmark::LinkType::Autolink;
use pulldown_cmark::Tag::*;
use pulldown_cmark::{CodeBlockKind, CowStr, Event};
use std::error::Error;
use std::io::prelude::*;
use std::path::Path;
use syntect::easy::HighlightLines;
use syntect::highlighting::Theme;
use url::Url;

/// State attributes for inline text.
#[derive(Debug, PartialEq)]
pub struct InlineAttrs {
    style: Style,
    indent: u16,
}

#[derive(Debug, PartialEq)]
pub enum InlineState {
    /// Inline text.
    ///
    /// Regular inline text without any particular implications.
    InlineText,
    /// Inline link.
    ///
    /// This state supresses link references being written when reading a link
    /// end event.
    InlineLink,
    /// A list item.
    ///
    /// Unlike other inline states this inline state permits immediate
    /// transition to block level when reading a paragraph begin event, which
    /// denotes a list with full paragraphs inside.
    ListItemText,
}

/// State attributes for styled blocks.
#[derive(Debug, PartialEq)]
pub struct StyledBlockAttrs {
    margin_before: bool,
    indent: u16,
    style: Style,
}

impl StyledBlockAttrs {
    fn with_margin_before(self) -> Self {
        StyledBlockAttrs {
            margin_before: true,
            ..self
        }
    }
}

#[derive(Debug, PartialEq)]
pub struct HighlightBlockAttrs<'a> {
    syntax_token: Option<CowStr<'a>>,
}

#[derive(Debug, PartialEq, Clone, Copy)]
enum ListItemType {
    Unordered,
    Ordered(u64),
}

#[derive(Debug, PartialEq)]
pub struct ListBlockAttrs {
    item_type: ListItemType,
    newline_before: bool,
    indent: u16,
    style: Style,
}

impl ListBlockAttrs {
    fn to_next_item(mut self) -> Self {
        self.item_type = match self.item_type {
            ListItemType::Unordered => ListItemType::Unordered,
            ListItemType::Ordered(start) => ListItemType::Ordered(start + 1),
        };
        self.newline_before = true;
        self
    }
}

#[derive(Debug, PartialEq)]
pub enum NestedState<'a> {
    /// Styled block.
    ///
    /// A block with attached style
    StyledBlock(StyledBlockAttrs),
    /// A highlighted block of code.
    HighlightBlock(HighlightBlockAttrs<'a>),
    /// A list.
    ListBlock(ListBlockAttrs),
    /// Some inline markup.
    Inline(InlineState, InlineAttrs),
}

/// State attributes for top level.
#[derive(Debug, PartialEq)]
pub struct TopLevelAttrs {
    margin_before: bool,
}

impl TopLevelAttrs {
    fn margin_before() -> Self {
        TopLevelAttrs {
            margin_before: true,
        }
    }
}

impl Default for TopLevelAttrs {
    fn default() -> Self {
        TopLevelAttrs {
            margin_before: false,
        }
    }
}

#[derive(Debug, PartialEq)]
pub enum State<'a> {
    /// At top level.
    TopLevel(TopLevelAttrs),
    /// A nested state, with a state to return to and the actual state.
    NestedState(Box<State<'a>>, NestedState<'a>),
}

impl<'a> Default for State<'a> {
    fn default() -> Self {
        State::TopLevel(TopLevelAttrs::default())
    }
}

#[derive(Debug, PartialEq)]
pub struct Link<'a> {
    index: u16,
    target: CowStr<'a>,
    title: CowStr<'a>,
}

#[derive(Debug)]
pub struct StateData<'a> {
    pending_links: Vec<Link<'a>>,
    next_link: u16,
}

impl<'a> StateData<'a> {
    fn add_link(mut self, target: CowStr<'a>, title: CowStr<'a>) -> (Self, u16) {
        let index = self.next_link;
        self.next_link += 1;
        self.pending_links.push(Link {
            index,
            target,
            title,
        });
        (self, index)
    }

    fn take_links(self) -> (Self, Vec<Link<'a>>) {
        let links = self.pending_links;
        (
            StateData {
                pending_links: Vec::new(),
                ..self
            },
            links,
        )
    }
}

impl<'a> Default for StateData<'a> {
    fn default() -> Self {
        StateData {
            pending_links: Vec::new(),
            next_link: 1,
        }
    }
}

#[inline]
fn write_indent<W: Write>(writer: &mut W, level: u16) -> std::io::Result<()> {
    write!(writer, "{}", " ".repeat(level as usize))
}

#[inline]
fn write_styled<W: Write, S: AsRef<str>>(
    writer: &mut W,
    capabilities: &TerminalCapabilities,
    style: &Style,
    text: S,
) -> std::io::Result<()> {
    match capabilities.style {
        StyleCapability::None => write!(writer, "{}", text.as_ref())?,
        StyleCapability::Ansi(ref ansi) => ansi.write_styled(writer, style, text)?,
    }
    Ok(())
}

fn write_mark<W: Write>(
    writer: &mut W,
    capabilities: &TerminalCapabilities,
) -> std::io::Result<()> {
    match capabilities.marks {
        MarkCapability::ITerm2(ref marks) => marks.set_mark(writer),
        MarkCapability::None => Ok(()),
    }
}

#[inline]
fn write_rule<W: Write>(
    writer: &mut W,
    capabilities: &TerminalCapabilities,
    length: usize,
) -> std::io::Result<()> {
    let rule = "\u{2550}".repeat(length);
    let style = Style::new().fg(Colour::Green);
    write_styled(writer, capabilities, &style, rule)
}

#[inline]
fn write_border<W: Write>(
    writer: &mut W,
    capabilities: &TerminalCapabilities,
    terminal_size: &TerminalSize,
) -> std::io::Result<()> {
    let separator = "\u{2500}".repeat(terminal_size.width.min(20));
    let style = Style::new().fg(Colour::Green);
    write_styled(writer, capabilities, &style, separator)?;
    writeln!(writer)
}

#[inline]
fn writeln_returning_to_toplevel<W: Write>(writer: &mut W, state: &State) -> std::io::Result<()> {
    match state {
        State::TopLevel(_) => writeln!(writer),
        _ => Ok(()),
    }
}

fn write_link_refs<W: Write>(
    writer: &mut W,
    capabilities: &TerminalCapabilities,
    links: Vec<Link>,
) -> std::io::Result<()> {
    if !links.is_empty() {
        writeln!(writer)?;
        let style = Style::new().fg(Colour::Blue);
        for link in links {
            let link_text = format!("[{}]: {} {}", link.index, link.target, link.title);
            write_styled(writer, capabilities, &style, link_text)?;
            writeln!(writer)?;
        }
    }
    Ok(())
}

pub fn write_event<'a, W: Write>(
    writer: &mut W,
    settings: &Settings,
    base_dir: &Path,
    theme: &Theme,
    state: State<'a>,
    data: StateData<'a>,
    event: Event<'a>,
) -> Result<(State<'a>, StateData<'a>), Box<dyn Error>> {
    use self::InlineState::*;
    use self::NestedState::*;
    use State::*;
    match (state, event) {
        (TopLevel(attrs), Start(Paragraph)) => {
            if attrs.margin_before {
                writeln!(writer)?;
            }
            Ok((
                NestedState(
                    Box::new(TopLevel(TopLevelAttrs::margin_before())),
                    Inline(
                        InlineText,
                        InlineAttrs {
                            style: Style::new(),
                            indent: 0,
                        },
                    ),
                ),
                data,
            ))
        }
        (TopLevel(attrs), Start(Heading(level))) => {
            let (data, links) = data.take_links();
            write_link_refs(writer, &settings.terminal_capabilities, links)?;
            if attrs.margin_before {
                writeln!(writer)?;
            }
            write_mark(writer, &settings.terminal_capabilities)?;
            let style = Style::new().fg(Colour::Blue).bold();
            write_styled(
                writer,
                &settings.terminal_capabilities,
                &style,
                "\u{2504}".repeat(level as usize),
            )?;
            Ok((
                NestedState(
                    Box::new(TopLevel(TopLevelAttrs::margin_before())),
                    Inline(InlineText, InlineAttrs { style, indent: 0 }),
                ),
                data,
            ))
        }
        (TopLevel(attrs), Start(BlockQuote)) => {
            if attrs.margin_before {
                writeln!(writer)?;
            }
            Ok((
                NestedState(
                    Box::new(TopLevel(TopLevelAttrs::margin_before())),
                    StyledBlock(StyledBlockAttrs {
                        // We've written a block-level margin already, so the first
                        // block inside the styled block should add another margin.
                        margin_before: false,
                        style: Style::new().italic().fg(Colour::Green),
                        indent: 4,
                    }),
                ),
                data,
            ))
        }
        (TopLevel(attrs), Rule) => {
            if attrs.margin_before {
                writeln!(writer)?;
            }
            write_rule(
                writer,
                &settings.terminal_capabilities,
                settings.terminal_size.width,
            )?;
            writeln!(writer)?;
            Ok((TopLevel(attrs), data))
        }
        (TopLevel(attrs), Start(CodeBlock(kind))) => {
            if attrs.margin_before {
                writeln!(writer)?;
            }
            write_border(
                writer,
                &settings.terminal_capabilities,
                &settings.terminal_size,
            )?;

            let syntax_token = match kind {
                CodeBlockKind::Indented => None,
                CodeBlockKind::Fenced(name) if name.is_empty() => None,
                CodeBlockKind::Fenced(name) => Some(name),
            };
            Ok((
                NestedState(
                    Box::new(TopLevel(TopLevelAttrs::margin_before())),
                    HighlightBlock(HighlightBlockAttrs { syntax_token }),
                ),
                data,
            ))
        }
        (TopLevel(attrs), Start(List(start))) => {
            if attrs.margin_before {
                writeln!(writer)?;
            }
            Ok((
                NestedState(
                    Box::new(TopLevel(TopLevelAttrs::margin_before())),
                    ListBlock(ListBlockAttrs {
                        item_type: start.map_or(ListItemType::Unordered, |start| {
                            ListItemType::Ordered(start)
                        }),
                        style: Style::new(),
                        newline_before: false,
                        indent: 0,
                    }),
                ),
                data,
            ))
        }

        // This is a somewhat special case: If we open a paragraph in inline list item text we're
        // at the beginning if a list item which contains multiple paragraphs.
        //
        // In this case we move to inline text to write the first line of the paragraph right away
        // beneath the list item bullet, and afterwards return to a styled block to tread subsequent
        // paragraph like regular nested blocks with style (e.g. as in a block quote).
        (NestedState(return_to, Inline(ListItemText, attrs)), Start(Paragraph)) => {
            let InlineAttrs { style, indent } = attrs;
            Ok((
                NestedState(
                    Box::new(NestedState(
                        return_to,
                        StyledBlock(StyledBlockAttrs {
                            margin_before: true,
                            style,
                            indent,
                        }),
                    )),
                    Inline(InlineText, attrs),
                ),
                data,
            ))
        }
        // This is similiar to a paragraph in inline list item text but we now start a nested list
        (NestedState(return_to, Inline(ListItemText, attrs)), Start(List(start))) => {
            // End the current list item text and give way to the new list.
            writeln!(writer)?;

            let InlineAttrs { style, indent } = attrs;
            Ok((
                NestedState(
                    Box::new(NestedState(return_to, Inline(ListItemText, attrs))),
                    ListBlock(ListBlockAttrs {
                        item_type: start.map_or(ListItemType::Unordered, |start| {
                            ListItemType::Ordered(start)
                        }),
                        newline_before: false,
                        indent,
                        style,
                    }),
                ),
                data,
            ))
        }

        (NestedState(return_to, StyledBlock(attrs)), Start(Paragraph)) => {
            if attrs.margin_before {
                writeln!(writer)?;
            }
            write_indent(writer, attrs.indent)?;
            let StyledBlockAttrs { style, indent, .. } = attrs;
            Ok((
                NestedState(
                    Box::new(NestedState(
                        return_to,
                        StyledBlock(attrs.with_margin_before()),
                    )),
                    Inline(InlineText, InlineAttrs { style, indent }),
                ),
                data,
            ))
        }
        (NestedState(return_to, StyledBlock(attrs)), Rule) => {
            if attrs.margin_before {
                writeln!(writer)?;
            }
            write_indent(writer, attrs.indent)?;
            write_rule(
                writer,
                &settings.terminal_capabilities,
                settings.terminal_size.width - (attrs.indent as usize),
            )?;
            writeln!(writer)?;
            Ok((
                NestedState(return_to, StyledBlock(attrs.with_margin_before())),
                data,
            ))
        }
        (NestedState(return_to, StyledBlock(attrs)), Start(Heading(level))) => {
            if attrs.margin_before {
                writeln!(writer)?;
            }
            write_indent(writer, attrs.indent)?;

            // We deliberately don't mark headings which aren't top-level.
            let style = attrs.style.bold();
            write_styled(
                writer,
                &settings.terminal_capabilities,
                &style,
                "\u{2504}".repeat(level as usize),
            )?;

            let indent = attrs.indent;
            Ok((
                NestedState(
                    Box::new(NestedState(
                        return_to,
                        StyledBlock(attrs.with_margin_before()),
                    )),
                    Inline(InlineText, InlineAttrs { style, indent }),
                ),
                data,
            ))
        }

        (NestedState(return_to, StyledBlock(attrs)), Start(List(start))) => {
            let StyledBlockAttrs {
                margin_before,
                style,
                indent,
            } = attrs;
            if margin_before {
                writeln!(writer)?;
            }
            Ok((
                NestedState(
                    Box::new(NestedState(return_to, StyledBlock(attrs))),
                    ListBlock(ListBlockAttrs {
                        item_type: start.map_or(ListItemType::Unordered, |start| {
                            ListItemType::Ordered(start)
                        }),
                        newline_before: false,
                        style,
                        indent,
                    }),
                ),
                data,
            ))
        }
        (NestedState(return_to, StyledBlock(_)), End(Item)) => Ok((*return_to, data)),

        (NestedState(return_to, HighlightBlock(attrs)), Text(text)) => {
            match settings.terminal_capabilities.style {
                StyleCapability::None => {
                    write!(writer, "{}", text)?;
                }
                StyleCapability::Ansi(ref ansi) => {
                    match attrs
                        .syntax_token
                        .as_ref()
                        .and_then(|token| settings.syntax_set.find_syntax_by_token(token))
                        .map(|syntax| HighlightLines::new(syntax, theme))
                    {
                        None => {
                            let style = Style::new().fg(Colour::Yellow);
                            write_styled(writer, &settings.terminal_capabilities, &style, text)?;
                        }
                        Some(mut highlighter) => {
                            let regions = highlighter.highlight(&text, &settings.syntax_set);
                            highlighting::write_as_ansi(writer, ansi, &regions)?;
                        }
                    }
                }
            }
            Ok((NestedState(return_to, HighlightBlock(attrs)), data))
        }
        (NestedState(return_to, HighlightBlock(_)), End(CodeBlock(_))) => {
            write_border(
                writer,
                &settings.terminal_capabilities,
                &settings.terminal_size,
            )?;
            Ok((*return_to, data))
        }

        (NestedState(return_to, _), End(BlockQuote)) => Ok((*return_to, data)),

        (NestedState(return_to, ListBlock(attrs)), Start(Item)) => {
            let ListBlockAttrs {
                style,
                indent,
                item_type,
                newline_before,
            } = attrs;

            if newline_before {
                writeln!(writer)?;
            }
            write_indent(writer, indent)?;

            let indent = match item_type {
                ListItemType::Unordered => {
                    write!(writer, "\u{2022} ")?;
                    indent + 2
                }
                ListItemType::Ordered(no) => {
                    write!(writer, "{:>2}. ", no)?;
                    indent + 4
                }
            };

            Ok((
                NestedState(
                    Box::new(NestedState(return_to, ListBlock(attrs.to_next_item()))),
                    Inline(ListItemText, InlineAttrs { style, indent }),
                ),
                data,
            ))
        }
        (NestedState(return_to, ListBlock(_)), End(List(_))) => {
            writeln_returning_to_toplevel(writer, &return_to)?;
            Ok((*return_to, data))
        }

        (NestedState(return_to, Inline(state, attrs)), Start(Emphasis)) => {
            let indent = attrs.indent;
            let style = Style {
                is_italic: !attrs.style.is_italic,
                ..attrs.style
            };
            Ok((
                NestedState(
                    Box::new(NestedState(return_to, Inline(state, attrs))),
                    Inline(InlineText, InlineAttrs { style, indent }),
                ),
                data,
            ))
        }
        (NestedState(return_to, Inline(_, _)), End(Emphasis)) => Ok((*return_to, data)),

        (NestedState(return_to, Inline(state, attrs)), Start(Strong)) => {
            let indent = attrs.indent;
            let style = attrs.style.bold();
            Ok((
                NestedState(
                    Box::new(NestedState(return_to, Inline(state, attrs))),
                    Inline(InlineText, InlineAttrs { style, indent }),
                ),
                data,
            ))
        }
        (NestedState(return_to, Inline(_, _)), End(Strong)) => Ok((*return_to, data)),

        (NestedState(return_to, Inline(state, attrs)), Start(Strikethrough)) => {
            let style = attrs.style.strikethrough();
            let indent = attrs.indent;
            Ok((
                NestedState(
                    Box::new(NestedState(return_to, Inline(state, attrs))),
                    Inline(InlineText, InlineAttrs { style, indent }),
                ),
                data,
            ))
        }
        (NestedState(return_to, Inline(_, _)), End(Strikethrough)) => Ok((*return_to, data)),

        (NestedState(return_to, Inline(state, attrs)), Code(code)) => {
            write_styled(
                writer,
                &settings.terminal_capabilities,
                &attrs.style.fg(Colour::Yellow),
                code,
            )?;
            Ok((NestedState(return_to, Inline(state, attrs)), data))
        }

        (NestedState(return_to, Inline(state, attrs)), SoftBreak) => {
            writeln!(writer)?;
            write_indent(writer, attrs.indent)?;
            Ok((NestedState(return_to, Inline(state, attrs)), data))
        }
        (NestedState(return_to, Inline(state, attrs)), HardBreak) => {
            writeln!(writer)?;
            write_indent(writer, attrs.indent)?;
            Ok((NestedState(return_to, Inline(state, attrs)), data))
        }

        (NestedState(return_to, Inline(state, attrs)), Text(text)) => {
            write_styled(writer, &settings.terminal_capabilities, &attrs.style, text)?;
            Ok((NestedState(return_to, Inline(state, attrs)), data))
        }

        (NestedState(return_to, Inline(InlineText, attrs)), Start(Link(_, target, _))) => {
            let indent = attrs.indent;
            let style = attrs.style.fg(Colour::Blue);
            match settings.terminal_capabilities.links {
                LinkCapability::OSC8(ref osc8) => {
                    // TODO: Handle mailto links
                    match Url::parse(&target)
                        .or_else(|_| Url::from_file_path(base_dir.join(target.as_ref())))
                        .ok()
                    {
                        Some(url) => {
                            osc8.set_link_url(writer, url)?;
                            Ok((
                                NestedState(
                                    Box::new(NestedState(return_to, Inline(InlineText, attrs))),
                                    Inline(InlineLink, InlineAttrs { style, indent }),
                                ),
                                data,
                            ))
                        }
                        None => Ok((
                            NestedState(
                                Box::new(NestedState(return_to, Inline(InlineText, attrs))),
                                Inline(InlineText, InlineAttrs { style, indent }),
                            ),
                            data,
                        )),
                    }
                }
                // If we can't write inline links continue with inline text;
                // we'll write a link reference on the End(Link) event.
                LinkCapability::None => {
                    let indent = attrs.indent;
                    let style = attrs.style.fg(Colour::Blue);
                    Ok((
                        NestedState(
                            Box::new(NestedState(return_to, Inline(InlineText, attrs))),
                            Inline(InlineText, InlineAttrs { style, indent }),
                        ),
                        data,
                    ))
                }
            }
        }
        (NestedState(return_to, Inline(InlineLink, _)), End(Link(_, _, _))) => {
            match settings.terminal_capabilities.links {
                LinkCapability::OSC8(ref osc8) => {
                    osc8.clear_link(writer)?;
                }
                LinkCapability::None => {
                    panic!("Unreachable code: We opened an inline link but can't close it now?")
                }
            }
            Ok((*return_to, data))
        }
        // When closing an autolink in inline text we just return because the link's
        // already written out (link text and link destination are identical for autolinks)
        (NestedState(return_to, Inline(InlineText, _)), End(Link(Autolink, _, _))) => {
            Ok((*return_to, data))
        }
        (NestedState(return_to, Inline(InlineText, attrs)), End(Link(_, target, title))) => {
            let (data, index) = data.add_link(target, title);
            write_styled(
                writer,
                &settings.terminal_capabilities,
                &attrs.style.fg(Colour::Blue),
                format!("[{}]", index),
            )?;
            Ok((*return_to, data))
        }

        (NestedState(return_to, Inline(ListItemText, _)), End(Item)) => Ok((*return_to, data)),
        (NestedState(return_to, Inline(_, _)), End(Paragraph)) => {
            writeln!(writer)?;
            Ok((*return_to, data))
        }
        (NestedState(return_to, Inline(_, _)), End(Heading(_))) => {
            writeln!(writer)?;
            Ok((*return_to, data))
        }

        // Impossible events
        (s @ TopLevel(_), e @ Code(_)) => impossible(s, e),
        (s @ TopLevel(_), e @ Text(_)) => impossible(s, e),

        // TODO: Remove and cover all impossible cases when finishing this branch.
        (s, e) => panic!("Unexpected event in state {:?}: {:?}", s, e),
    }
}

#[inline]
fn impossible(state: State, event: Event) -> ! {
    panic!(
        "Event {:?} impossible in state {:?}

Please do report an issue at <https://github.com/lunaryorn/mdcat/issues/new> including

* a copy of this message, and
* the markdown document which caused this error.",
        state, event
    )
}

pub fn finish<'a, W: Write>(
    writer: &mut W,
    settings: &Settings,
    state: State,
    data: StateData<'a>,
) -> Result<(), Box<dyn Error>> {
    match state {
        State::TopLevel(_) => {
            write_link_refs(writer, &settings.terminal_capabilities, data.pending_links)?;
            Ok(())
        }
        _ => {
            panic!("Must finish in state TopLevel but got: {:?}", state);
        }
    }
}
