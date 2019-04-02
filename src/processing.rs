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

mod events;

pub use self::events::*;
