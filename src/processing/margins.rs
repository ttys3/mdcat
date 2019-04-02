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

use super::PassEvent;
use super::PassEvent::*;
use super::PrintEvent::*;
use pulldown_cmark::Event::*;
use pulldown_cmark::Tag::*;

/// Inject margins into a stream of events
pub fn inject_margins<'a, I>(events: I) -> impl Iterator<Item = PassEvent<'a>>
where
    I: Iterator<Item = PassEvent<'a>>,
{
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
