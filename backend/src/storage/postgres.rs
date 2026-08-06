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

use super::error::StorageError;
use crate::worker::do_work::{CheckEmailJobId, CheckEmailTask, TaskError};
use check_if_email_exists::{CheckEmailOutput, LOG_TARGET};
use serde::Serialize;
use sqlx::postgres::PgPoolOptions;
use sqlx::{PgPool, Row};
use tracing::{debug, info};

#[derive(Debug)]
pub struct PostgresStorage {
	pub pg_pool: PgPool,
	extra: Option<serde_json::Value>,
}

impl PostgresStorage {
	pub async fn new(db_url: &str, extra: Option<serde_json::Value>) -> Result<Self, StorageError> {
		debug!(target: LOG_TARGET, "Connecting to DB: {}", db_url);
		// create connection pool with database
		// connection pool internally the shared db connection
		// with arc so it can safely be cloned and shared across threads
		let pg_pool = PgPoolOptions::new().connect(db_url).await?;

		sqlx::migrate!("./migrations").run(&pg_pool).await?;

		info!(target: LOG_TARGET, table="v1_task_result", "Connected to DB, Reacher will write verification results to DB");

		Ok(Self { pg_pool, extra })
	}

	pub async fn store(
		&self,
		task: &CheckEmailTask,
		worker_output: &Result<CheckEmailOutput, TaskError>,
		extra: Option<serde_json::Value>,
	) -> Result<(), StorageError> {
		let payload_json = serde_json::to_value(task)?;

		match worker_output {
			Ok(output) => {
				let output_json = serde_json::to_value(output)?;

				sqlx::query!(
					r#"
					INSERT INTO v1_task_result (payload, job_id, extra, result)
					VALUES ($1, $2, $3, $4)
					RETURNING id
					"#,
					payload_json,
					match task.job_id {
						CheckEmailJobId::Bulk(job_id) => Some(job_id),
						CheckEmailJobId::SingleShot => None,
					},
					extra,
					output_json,
				)
				.fetch_one(&self.pg_pool)
				.await?;
			}
			Err(err) => {
				sqlx::query!(
					r#"
					INSERT INTO v1_task_result (payload, job_id, extra, error)
					VALUES ($1, $2, $3, $4)
					RETURNING id
					"#,
					payload_json,
					match task.job_id {
						CheckEmailJobId::Bulk(job_id) => Some(job_id),
						CheckEmailJobId::SingleShot => None,
					},
					extra,
					err.to_string(),
				)
				.fetch_one(&self.pg_pool)
				.await?;
			}
		}

		debug!(target: LOG_TARGET, email=?task.input.to_email, "Wrote to DB");

		Ok(())
	}

	pub fn get_extra(&self) -> Option<serde_json::Value> {
		self.extra.clone()
	}
}

/// A single blocked-address record returned by the analytics endpoint.
#[derive(Debug, Serialize)]
pub struct BlockedRecord {
	pub email: Option<String>,
	/// Why the address is considered blocked: "syntax", "disposable" or "invalid".
	pub reason: Option<String>,
	pub is_reachable: Option<String>,
	pub is_disposable: Option<bool>,
	pub is_valid_syntax: Option<bool>,
	pub created_at: String,
}

/// Result of a blocked-address analytics query: the total `count` matching the
/// filter (ignoring pagination) and the paginated `results`.
#[derive(Debug, Serialize)]
pub struct BlockedAnalytics {
	pub count: i64,
	pub results: Vec<BlockedRecord>,
}

/// SQL predicate selecting "blocked" rows for the given `reason` filter.
///
/// A row is blocked when the consumer-side validation would reject it:
/// invalid syntax, a disposable/tempmail domain, or a non-existent mailbox.
/// The `reason` value is validated by the caller against a fixed set, so it is
/// safe to inline the returned fragment into the SQL string.
fn blocked_predicate(reason: &str) -> &'static str {
	match reason {
		"disposable" => "(result->'misc'->>'is_disposable') = 'true'",
		"invalid" => "(result->>'is_reachable') = 'invalid'",
		"syntax" => "(result->'syntax'->>'is_valid_syntax') = 'false'",
		// "all"
		_ => "((result->'syntax'->>'is_valid_syntax') = 'false' \
		       OR (result->'misc'->>'is_disposable') = 'true' \
		       OR (result->>'is_reachable') = 'invalid')",
	}
}

/// Count and list the addresses that were blocked in the given time window.
///
/// Reads from the `v1_task_result` table, which Reacher populates for every
/// verification when Postgres storage is enabled. `from`/`to` are optional
/// RFC3339 timestamps (defaults: epoch .. now). `reason` must be one of
/// "all", "disposable", "invalid", "syntax".
pub async fn get_blocked_analytics(
	pool: &PgPool,
	from: Option<String>,
	to: Option<String>,
	reason: &str,
	limit: i64,
	offset: i64,
) -> Result<BlockedAnalytics, sqlx::Error> {
	let where_clause = format!(
		"result IS NOT NULL \
		 AND created_at >= COALESCE($1::timestamptz, 'epoch'::timestamptz) \
		 AND created_at <  COALESCE($2::timestamptz, now()) \
		 AND {}",
		blocked_predicate(reason)
	);

	let count: i64 =
		sqlx::query_scalar(&format!("SELECT COUNT(*) FROM v1_task_result WHERE {where_clause}"))
			.bind(from.clone())
			.bind(to.clone())
			.fetch_one(pool)
			.await?;

	let select_sql = format!(
		"SELECT \
		   COALESCE(result->>'input', payload->'input'->>'to_email') AS email, \
		   CASE \
		     WHEN (result->'syntax'->>'is_valid_syntax') = 'false' THEN 'syntax' \
		     WHEN (result->'misc'->>'is_disposable') = 'true' THEN 'disposable' \
		     WHEN (result->>'is_reachable') = 'invalid' THEN 'invalid' \
		   END AS reason, \
		   result->>'is_reachable' AS is_reachable, \
		   (result->'misc'->>'is_disposable')::boolean AS is_disposable, \
		   (result->'syntax'->>'is_valid_syntax')::boolean AS is_valid_syntax, \
		   created_at::text AS created_at \
		 FROM v1_task_result \
		 WHERE {where_clause} \
		 ORDER BY created_at DESC \
		 LIMIT $3 OFFSET $4"
	);

	let rows = sqlx::query(&select_sql)
		.bind(from)
		.bind(to)
		.bind(limit)
		.bind(offset)
		.fetch_all(pool)
		.await?;

	let results = rows
		.iter()
		.map(|row| {
			Ok(BlockedRecord {
				email: row.try_get("email")?,
				reason: row.try_get("reason")?,
				is_reachable: row.try_get("is_reachable")?,
				is_disposable: row.try_get("is_disposable")?,
				is_valid_syntax: row.try_get("is_valid_syntax")?,
				created_at: row.try_get("created_at")?,
			})
		})
		.collect::<Result<Vec<_>, sqlx::Error>>()?;

	Ok(BlockedAnalytics { count, results })
}
