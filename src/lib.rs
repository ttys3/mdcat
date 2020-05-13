// Copyright 2018-2020 Sebastian Wiesner <sebastian@swsnr.de>

// Licensed under the Apache License, Version 2.0 (the "License");
// you may not use this file except in compliance with the License.
// You may obtain a copy of the License at

// 	http://www.apache.org/licenses/LICENSE-2.0

// Unless required by applicable law or agreed to in writing, software
// distributed under the License is distributed on an "AS IS" BASIS,
// WITHOUT WARRANTIES OR CONDITIONS OF ANY KIND, either express or implied.
// See the License for the specific language governing permissions and
// limitations under the License.

#![deny(warnings, missing_docs, clippy::all)]

//! Write markdown to TTYs.

use pulldown_cmark::Event;
use std::error::Error;
use std::io::Write;
use std::path::Path;
use syntect::highlighting::ThemeSet;
use syntect::parsing::SyntaxSet;

mod magic;
mod resources;
mod svg;
mod terminal;

mod context_write;
mod state_write;

// Expose some select things for use in main
pub use crate::resources::ResourceAccess;
pub use crate::terminal::*;

/// Dump markdown events to a writer.
pub fn dump_events<'a, W, I>(writer: &mut W, events: I) -> Result<(), Box<dyn Error>>
where
    I: Iterator<Item = Event<'a>>,
    W: Write,
{
    for event in events {
        writeln!(writer, "{:?}", event)?;
    }
    Ok(())
}

/// Settings for markdown rendering.
#[derive(Debug)]
pub struct Settings {
    /// Capabilities of the terminal mdcat writes to.
    pub terminal_capabilities: TerminalCapabilities,
    /// The size of the terminal mdcat writes to.
    pub terminal_size: TerminalSize,
    /// Whether remote resource access is permitted.
    pub resource_access: ResourceAccess,
    /// Syntax set for syntax highlighting of code blocks.
    pub syntax_set: SyntaxSet,
}

/// Write markdown to a TTY.
///
/// Iterate over Markdown AST `events`, format each event for TTY output and
/// write the result to a `writer`, using the given `settings` for rendering and
/// resource access.  `base_dir` denotes the base directory the `events` were
/// read from, to resolve relative references in the Markdown document.
///
/// `push_tty` tries to limit output to the given number of TTY `columns` but
/// does not guarantee that output stays within the column limit.
pub fn push_tty<'a, 'e, W, I>(
    settings: &Settings,
    writer: &'a mut W,
    base_dir: &'a Path,
    mut events: I,
) -> Result<(), Box<dyn Error>>
where
    I: Iterator<Item = Event<'e>>,
    W: Write,
{
    let theme = &ThemeSet::load_defaults().themes["Solarized (dark)"];
    if cfg!(context_write) {
        use context_write::*;
        events
            .try_fold(Context::new(writer, settings, base_dir, theme), write_event)?
            .write_pending_links()?;
    } else {
        use state_write::*;
        let (final_state, final_data) = events.try_fold(
            (State::default(), StateData::default()),
            |(state, data), event| {
                write_event(writer, settings, base_dir, &theme, state, data, event)
            },
        )?;
        finish(writer, settings, final_state, final_data)?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use pulldown_cmark::Parser;

    fn render_string(input: &str, settings: &Settings) -> Result<String, Box<dyn Error>> {
        let source = Parser::new(input);
        let mut sink = Vec::new();
        push_tty(settings, &mut sink, &Path::new("/"), source)?;
        Ok(String::from_utf8_lossy(&sink).into())
    }

    mod layout {
        use super::render_string;
        use crate::*;
        use pretty_assertions::assert_eq;
        use std::error::Error;
        use syntect::parsing::SyntaxSet;

        fn render(markup: &str) -> Result<String, Box<dyn Error>> {
            render_string(
                markup,
                &Settings {
                    resource_access: ResourceAccess::LocalOnly,
                    syntax_set: SyntaxSet::default(),
                    terminal_capabilities: TerminalCapabilities::none(),
                    terminal_size: TerminalSize::default(),
                },
            )
        }

        #[test]
        #[allow(non_snake_case)]
        fn GH_49_format_no_colour_simple() {
            assert_eq!(
                render("_lorem_ **ipsum** dolor **sit** _amet_").unwrap(),
                "lorem ipsum dolor sit amet\n",
            )
        }

        #[test]
        fn begins_with_rule() {
            assert_eq!(render("----").unwrap(), "════════════════════════════════════════════════════════════════════════════════\n")
        }

        #[test]
        fn begins_with_block_quote() {
            assert_eq!(render("> Hello World").unwrap(), "    Hello World\n")
        }

        #[test]
        fn rule_in_block_quote() {
            assert_eq!(
                render(
                    "> Hello World

> ----"
                )
                .unwrap(),
                "    Hello World

    ════════════════════════════════════════════════════════════════════════════\n"
            )
        }

        #[test]
        fn heading_in_block_quote() {
            assert_eq!(
                render(
                    "> Hello World

> # Hello World"
                )
                .unwrap(),
                "    Hello World

    ┄Hello World\n"
            )
        }

        #[test]
        fn heading_levels() {
            assert_eq!(
                render(
                    "
# First

## Second

### Third"
                )
                .unwrap(),
                "┄First

┄┄Second

┄┄┄Third\n"
            )
        }

        #[test]
        fn autolink_creates_no_reference() {
            assert_eq!(
                render("Hello <http://example.com>").unwrap(),
                "Hello http://example.com\n"
            )
        }

        #[test]
        fn flush_ref_links_before_toplevel_heading() {
            assert_eq!(
                render(
                    "> Hello [World](http://example.com/world)

> # No refs before this headline

# But before this"
                )
                .unwrap(),
                "    Hello World[1]

    ┄No refs before this headline

[1]: http://example.com/world \n
┄But before this\n"
            )
        }

        #[test]
        fn flush_ref_links_at_end() {
            assert_eq!(
                render(
                    "Hello [World](http://example.com/world)

# Headline

Hello [Donald](http://example.com/Donald)"
                )
                .unwrap(),
                "Hello World[1]

[1]: http://example.com/world \n
┄Headline

Hello Donald[2]

[2]: http://example.com/Donald \n"
            )
        }
    }
}
