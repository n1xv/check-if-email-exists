// check-if-email-exists
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

mod gravatar;
use crate::haveibeenpwned::check_haveibeenpwned;
use crate::syntax::SyntaxDetails;
use gravatar::check_gravatar;
use once_cell::sync::Lazy;
use serde::{Deserialize, Serialize};
use std::{collections::HashSet, default::Default};
use thiserror::Error;

const ROLE_ACCOUNTS: &str = include_str!("./roles.txt");
const FREE_EMAIL_PROVIDERS: &str = include_str!("./b2c.txt");
const DISPOSABLE_DOMAINS: &str = include_str!("./disposable.txt");

// Lazy static initialization of domain sets
static ROLE_ACCOUNTS_SET: Lazy<HashSet<String>> = Lazy::new(|| load_str_as_hashset(ROLE_ACCOUNTS));
static FREE_EMAIL_PROVIDERS_SET: Lazy<HashSet<String>> =
	Lazy::new(|| load_str_as_hashset(FREE_EMAIL_PROVIDERS));
// Our own curated disposable-domain denylist, on top of the `mailchecker`
// crate. See `./disposable.txt` for the format.
static DISPOSABLE_DOMAINS_SET: Lazy<HashSet<String>> =
	Lazy::new(|| load_domains_as_hashset(DISPOSABLE_DOMAINS));

// Function to load a file with `\n`-separated lines into a HashSet.
fn load_str_as_hashset(file_content: &str) -> HashSet<String> {
	file_content
		.lines()
		.map(|line| line.trim().to_string())
		.collect()
}

// Load a `\n`-separated list of domains into a HashSet, lowercasing each entry
// and skipping blank lines and `#` comments.
fn load_domains_as_hashset(file_content: &str) -> HashSet<String> {
	file_content
		.lines()
		.map(|line| line.trim().to_lowercase())
		.filter(|line| !line.is_empty() && !line.starts_with('#'))
		.collect()
}

/// Whether the given domain is on our custom disposable-domain denylist.
fn is_custom_disposable(domain: &str) -> bool {
	DISPOSABLE_DOMAINS_SET.contains(&domain.to_lowercase())
}

/// Miscellaneous details about the email address.
#[derive(Debug, Default, Deserialize, Serialize)]
pub struct MiscDetails {
	/// Is this a DEA (disposable email account)?
	pub is_disposable: bool,
	/// Is this email a role-based account?
	pub is_role_account: bool,
	/// Is this email a B2C email address?
	pub is_b2c: bool,
	/// If set, the gravatar URL for this email address.
	pub gravatar_url: Option<String>,
	/// Is this email address listed in the haveibeenpwned database for
	/// previous breaches?
	pub haveibeenpwned: Option<bool>,
}

/// Error occurred connecting to this email server via SMTP. Right now this
/// enum has no variant, as `check_misc` cannot fail. But putting a placeholder
/// right now to avoid future breaking changes.
#[derive(Debug, Error, Serialize)]
#[serde(tag = "type", content = "message")]
pub enum MiscError {}

/// Fetch misc details about the email address, such as whether it's disposable.
pub async fn check_misc(
	syntax: &SyntaxDetails,
	cfg_check_gravatar: bool,
	haveibeenpwned_api_key: Option<String>,
) -> MiscDetails {
	let address = syntax
		.address
		.as_ref()
		.expect("We already checked that the syntax was valid. qed.")
		.to_string();

	let mut gravatar_url: Option<String> = None;

	if cfg_check_gravatar {
		gravatar_url = check_gravatar(address.as_ref()).await;
	}

	let mut haveibeenpwned: Option<bool> = None;

	if haveibeenpwned_api_key.is_some() {
		haveibeenpwned = check_haveibeenpwned(address.as_ref(), haveibeenpwned_api_key).await;
	}

	MiscDetails {
		// mailchecker::is_valid checks also if the syntax is valid. But if
		// we're here, it means we're sure the syntax is valid, so is_valid
		// actually will only check if it's disposable. On top of mailchecker,
		// we also check our own curated disposable-domain denylist.
		is_disposable: !mailchecker::is_valid(address.as_ref())
			|| is_custom_disposable(&syntax.domain),
		is_role_account: ROLE_ACCOUNTS_SET.contains(&syntax.username.to_lowercase()),
		is_b2c: FREE_EMAIL_PROVIDERS_SET.contains(&syntax.domain.to_lowercase()),
		gravatar_url,
		haveibeenpwned,
	}
}
#[cfg(test)]
mod tests {
	use std::str::FromStr;

	use super::*;
	use crate::{syntax::SyntaxDetails, EmailAddress};

	#[tokio::test]
	async fn test_check_misc() {
		let syntax = SyntaxDetails {
			address: Some(EmailAddress::from_str("test@gmail.com").unwrap()),
			is_valid_syntax: true,
			username: "test".to_string(),
			domain: "gmail.com".to_string(),
			normalized_email: None,
			suggestion: None,
		};

		let misc_details = check_misc(&syntax, true, None).await;

		assert!(!misc_details.is_disposable); // gmail.com is not in mailchecker
		assert!(misc_details.is_role_account); // test is in roles.txt
		assert!(misc_details.is_b2c); // gmail.com is in b2c.txt
	}

	#[tokio::test]
	async fn test_custom_disposable_domain() {
		// zeteex.cfd is not known to mailchecker, but is on our custom
		// disposable.txt denylist, so it must be flagged as disposable.
		let syntax = SyntaxDetails {
			address: Some(EmailAddress::from_str("someone@zeteex.cfd").unwrap()),
			is_valid_syntax: true,
			username: "someone".to_string(),
			domain: "zeteex.cfd".to_string(),
			normalized_email: None,
			suggestion: None,
		};

		let misc_details = check_misc(&syntax, false, None).await;

		assert!(misc_details.is_disposable);
	}

	#[test]
	fn test_is_custom_disposable_is_case_insensitive() {
		assert!(is_custom_disposable("ZeTeEx.CFD"));
		assert!(!is_custom_disposable("gmail.com"));
	}
}
