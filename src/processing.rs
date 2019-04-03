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

//! Process markdown for TTY printing.
//!
//! This module and its submodule process a stream of Markdown events into a stream of what we call
//! "print events" which turn Markdown into a line-oriented document for printing, and ultimately
//! into a series of "style strings" which we can finally print to a TTY.
//!
//! Processing happens in multiple passes, each of which turns a certain kind of Markdown events
//! into print events.  Each pass runs as a lazy iterator; while we sometimes do need to drag state
//! along the events we try to retain the streaming interface of pulldown cmark.

use pulldown_cmark::Event::*;
use pulldown_cmark::Tag::*;
use pulldown_cmark::{CowStr, Event};

/// An event for printing to TTY.
#[derive(Debug)]
pub enum PrintEvent<'a> {
    /// A text to print.
    PlainText(CowStr<'a>),
    /// A margin at the end of block elements
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

/// Lift raw markdown events into pass events.
pub fn lift_events<'a, I>(events: I) -> impl Iterator<Item = PassEvent<'a>>
where
    I: Iterator<Item = Event<'a>>,
{
    events.map(PassEvent::Markdown)
}

/// Inject margins into a stream of events
pub fn inject_margins<'a, I>(events: I) -> impl Iterator<Item = PassEvent<'a>>
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

pub fn text_to_plaintext<'a, I>(events: I) -> impl Iterator<Item = PassEvent<'a>>
where
    I: Iterator<Item = PassEvent<'a>>,
{
    events.map(|e| match e {
        Markdown(Text(s)) => Print(PrintEvent::PlainText(s)),
        _ => e,
    })
}

pub fn remove_inline_markup<'a, I>(events: I) -> impl Iterator<Item = PassEvent<'a>>
where
    I: Iterator<Item = PassEvent<'a>>,
{
    events.filter(|e| match e {
        Markdown(Start(t)) | Markdown(End(t)) => match t {
            Strikethrough | Strong | Emphasis | Code => false,
            _ => true,
        },
        _ => true,
    })
}

/// Process Markdown events for printing.
///
/// Combines all passes in proper order.
pub fn process<'a, I>(events: I) -> impl Iterator<Item = PassEvent<'a>>
where
    I: Iterator<Item = Event<'a>>,
{
    remove_inline_markup(text_to_plaintext(inject_margins(lift_events(events))))
}
