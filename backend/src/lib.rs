// Reacher - Email Verification
// Copyright (C) 2018-2023 Reacher

// This program is free software: you can redistribute it and/or modify
// it under the terms of the GNU Affero General Public License as published
// by the Free Software Foundation, either version 3 of the License, or
// (at your option) any later version.

// This program is distributed in the hope that it will be useful,
// but WITHOUT ANY WARRANTY; without even the implied warranty of
// MERCHANTABILITY or FITNESS FOR A PARTICULAR PURPOSE.  See the
// GNU Affero General Public License for more details.

// You should have received a copy of the GNU Affero General Public License
// along with this program.  If not, see <https://www.gnu.org/licenses/>.

// The warp filter chain in `http::create_routes` builds a deeply nested
// generic type. Adding routes pushes the trait solver past the default
// recursion limit of 128 when it evaluates Send/Sync bounds, so we raise it.
#![recursion_limit = "512"]

pub mod config;
pub mod http;
pub mod storage;
pub mod throttle;
pub mod worker;

const CARGO_PKG_VERSION: &str = env!("CARGO_PKG_VERSION");
