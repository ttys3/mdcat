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

use pulldown_cmark::{CowStr, Event};
use syntect::parsing::Scope;

/// An event for printing to TTY.
#[derive(Debug)]
pub enum PrintEvent<'a> {
    /// A text to print, with its highlighting scope.
    ScopedText(Vec<Scope>, CowStr<'a>),
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

/// Lift raw markdown events into pass events.
pub fn lift_events<'a, I>(events: I) -> impl Iterator<Item = PassEvent<'a>>
where
    I: Iterator<Item = Event<'a>>,
{
    events.map(PassEvent::Markdown)
}
