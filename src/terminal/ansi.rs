// Copyright 2018-2019 Sebastian Wiesner <sebastian@swsnr.de>

// Licensed under the Apache License, Version 2.0 (the "License");
// you may not use this file except in compliance with the License.
// You may obtain a copy of the License at

// 	http://www.apache.org/licenses/LICENSE-2.0

// Unless required by applicable law or agreed to in writing, software
// distributed under the License is distributed on an "AS IS" BASIS,
// WITHOUT WARRANTIES OR CONDITIONS OF ANY KIND, either express or implied.
// See the License for the specific language governing permissions and
// limitations under the License.

//! Standard ANSI styling.

use std::io::{Result, Write};
use syntect::highlighting::Color;
use syntect::highlighting::{FontStyle, Style};

#[inline]
fn to_colour(color: &Color) -> ansi_term::Colour {
    ansi_term::Colour::RGB(color.r, color.g, color.b)
}

/// Convert a highlighting style to an ANSI style for printing.
///
/// We deliberately ignore the background color: To make background colors look well we'd have to
/// make them cover the whole line with filling spaces, etc. which gets really weird.  bat doesn't
/// draw the background color either, and it's probably a good idea to follow their path here.
pub fn to_ansi(style: &Style) -> ansi_term::Style {
    let mut ansi_style = ansi_term::Style::new();
    ansi_style.foreground = Some(to_colour(&style.foreground));
    ansi_style.is_bold = style.font_style.contains(FontStyle::BOLD);
    ansi_style.is_italic = style.font_style.contains(FontStyle::ITALIC);
    ansi_style.is_underline = style.font_style.contains(FontStyle::UNDERLINE);
    ansi_style
}

/// Access to a terminal’s basic ANSI styling functionality.
pub struct AnsiStyle;

impl AnsiStyle {
    /// Write styled text to the given writer.
    pub fn write_styled<W: Write, V: AsRef<str>>(
        &self,
        write: &mut W,
        style: &Style,
        text: V,
    ) -> Result<()> {
        write!(write, "{}", to_ansi(style).paint(text.as_ref()))
    }
}
