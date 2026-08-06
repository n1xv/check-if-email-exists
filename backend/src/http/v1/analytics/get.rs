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

//! This file implements the `GET /v1/analytics/blocked` endpoint, which returns
//! the count and list of addresses that would be blocked (disposable / invalid
//! syntax / non-existent mailbox) over a given time period.
//!
//! Requires Postgres storage (`RCH__STORAGE__POSTGRES__DB_URL`); it reads from
//! the `v1_task_result` table that Reacher populates on every verification.

use std::sync::Arc;

use check_if_email_exists::LOG_TARGET;
use serde::Deserialize;
use tracing::info;
use warp::{http, Filter};

use crate::config::BackendConfig;
use crate::http::v0::check_email::post::with_config;
use crate::http::{check_header, ReacherResponseError};
use crate::storage::postgres::get_blocked_analytics;

/// Query parameters for `GET /v1/analytics/blocked`.
#[derive(Debug, Deserialize)]
struct AnalyticsQuery {
	/// Inclusive lower bound, RFC3339 (default: epoch).
	from: Option<String>,
	/// Exclusive upper bound, RFC3339 (default: now).
	to: Option<String>,
	/// One of "all" (default), "disposable", "invalid", "syntax".
	reason: Option<String>,
	/// Max rows to return (default 100, capped at 10000).
	limit: Option<i64>,
	/// Rows to skip for pagination (default 0).
	offset: Option<i64>,
}

async fn http_handler(
	config: Arc<BackendConfig>,
	query: AnalyticsQuery,
) -> Result<impl warp::Reply, warp::Rejection> {
	let reason = query.reason.as_deref().unwrap_or("all");
	if !matches!(reason, "all" | "disposable" | "invalid" | "syntax") {
		return Err(ReacherResponseError::new(
			http::StatusCode::BAD_REQUEST,
			"reason must be one of: all, disposable, invalid, syntax",
		)
		.into());
	}
	let limit = query.limit.unwrap_or(100).clamp(1, 10_000);
	let offset = query.offset.unwrap_or(0).max(0);

	// Analytics needs the Postgres-backed history.
	let pool = match config.get_pg_pool() {
		Some(pool) => pool,
		None => {
			return Err(ReacherResponseError::new(
				http::StatusCode::NOT_IMPLEMENTED,
				"Analytics requires Postgres storage. Set RCH__STORAGE__POSTGRES__DB_URL.",
			)
			.into());
		}
	};

	let analytics = get_blocked_analytics(
		&pool,
		query.from.clone(),
		query.to.clone(),
		reason,
		limit,
		offset,
	)
	.await
	.map_err(ReacherResponseError::from)?;

	info!(target: LOG_TARGET, count=analytics.count, reason=reason, "Blocked analytics query");

	let body = serde_json::json!({
		"count": analytics.count,
		"from": query.from,
		"to": query.to,
		"reason": reason,
		"limit": limit,
		"offset": offset,
		"results": analytics.results,
	});

	Ok(warp::reply::json(&body))
}

/// Create the `GET /v1/analytics/blocked` endpoint.
pub fn v1_analytics_blocked(
	config: Arc<BackendConfig>,
) -> impl Filter<Extract = (impl warp::Reply,), Error = warp::Rejection> + Clone {
	warp::path!("v1" / "analytics" / "blocked")
		.and(warp::get())
		.and(check_header(Arc::clone(&config)))
		.and(with_config(config.clone()))
		.and(warp::query::<AnalyticsQuery>())
		.and_then(http_handler)
		// View access logs by setting `RUST_LOG=reacher`.
		.with(warp::log(LOG_TARGET))
}
