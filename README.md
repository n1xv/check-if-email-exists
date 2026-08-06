# Email Verification Service

Self-hosted HTTP API to **validate email addresses** — reject temporary
(disposable) and invalid addresses at signup/input, and see **how many
addresses were blocked** over any period.

Forked from [Reacher / `check-if-email-exists`](https://github.com/reacherhq/check-if-email-exists)
with three additions on top of upstream:

- **Custom disposable-domain denylist** (`core/src/misc/disposable.txt`) merged into `misc.is_disposable`, on top of the built-in `mailchecker` list.
- **Allowlist** (`core/src/misc/disposable_allowlist.txt`) — domains that must never be treated as disposable (e.g. `mailinator.com` for testing), overriding both sources.
- **Analytics endpoint** `GET /v1/analytics/blocked` — count + list of blocked addresses over a time period.

This document is about **integrating with the running service**. For running/deploying
the backend itself, see [`backend/README.md`](./backend/README.md).

---

## 🔑 Credentials & base URL

**This repository is public — no URLs, secrets or passwords are committed here.**
Get them from **Railway**:

| Value | Where in Railway |
| --- | --- |
| **Base URL** | The Reacher service → **Settings → Networking** → the generated domain (`https://<name>.up.railway.app`). |
| **Auth secret** | The Reacher service → **Variables** → `RCH__HEADER_SECRET`. |
| **Postgres URL** (analytics only) | The Postgres service → **Variables** → `DATABASE_URL`. |

Every request must send the secret in the **`x-reacher-secret`** header.

In your app, store them as environment variables (never hard-code, never expose to the browser):

```bash
REACHER_URL=...      # the Railway domain
REACHER_SECRET=...   # value of RCH__HEADER_SECRET
```

---

## 📡 Endpoints

### `POST /v1/check_email` — verify one address

Request:

```http
POST /v1/check_email
x-reacher-secret: <REACHER_SECRET>
Content-Type: application/json

{ "to_email": "someone@gmail.com" }
```

Relevant response fields:

```json
{
  "input": "someone@gmail.com",
  "is_reachable": "safe",              // safe | risky | invalid | unknown
  "misc":   { "is_disposable": false, "is_role_account": false },
  "mx":     { "accepts_mail": true },
  "smtp":   { "is_deliverable": true, "can_connect_smtp": true, "is_catch_all": false },
  "syntax": { "is_valid_syntax": true, "domain": "gmail.com" }
}
```

### `GET /v1/analytics/blocked` — blocked-address analytics

Returns the count and list of addresses that would be rejected (disposable / invalid
syntax / non-existent mailbox) in a time window. **Requires Postgres storage** (see below).

Query params (all optional):

| Param | Meaning | Default |
| --- | --- | --- |
| `from` | RFC3339, inclusive lower bound | epoch |
| `to` | RFC3339, exclusive upper bound | now |
| `reason` | `all` \| `disposable` \| `invalid` \| `syntax` | `all` |
| `limit` | max rows (1–10000) | 100 |
| `offset` | pagination offset | 0 |

Response:

```json
{
  "count": 1234,
  "from": "2026-08-01T00:00:00Z",
  "to": "2026-09-01T00:00:00Z",
  "reason": "all",
  "limit": 100,
  "offset": 0,
  "results": [
    { "email": "foo@zeteex.cfd", "reason": "disposable",
      "is_reachable": "risky", "is_disposable": true,
      "is_valid_syntax": true, "created_at": "2026-08-06 13:54:24+00" }
  ]
}
```

---

## 🧩 How to integrate

**Golden rule: call this service only from your backend.** The secret must never
reach the browser or a client bundle. Flow: `frontend → your backend → this service`.

### Decision logic (what to reject)

| Condition (from `check_email`) | Action | Suggested message |
| --- | --- | --- |
| `syntax.is_valid_syntax == false` | **reject** | "Please enter a valid email" |
| `misc.is_disposable == true` | **reject** | "We don't support temporary (disposable) email addresses" |
| `is_reachable == "invalid"` | **reject** | "This email address doesn't seem to exist" |
| `is_reachable` is `safe` or `risky` | **accept** | — |
| `is_reachable == "unknown"` | **accept** (fail-open) | — |
| timeout / service error | **accept** (fail-open) | log for monitoring |

Why fail-open on `unknown`/errors: some providers (Outlook/Hotmail/Yahoo) can't be
verified from a cloud IP and legitimately return `unknown` — blocking them would lock
out real users. Likewise, the checker being down must not break signups.

### Quickstart (curl)

```bash
# verify an address
curl -X POST "$REACHER_URL/v1/check_email" \
  -H "x-reacher-secret: $REACHER_SECRET" \
  -H "Content-Type: application/json" \
  -d '{"to_email":"someone@gmail.com"}'

# how many blocked this month + the list
curl "$REACHER_URL/v1/analytics/blocked?from=2026-08-01T00:00:00Z&to=2026-09-01T00:00:00Z&limit=1000" \
  -H "x-reacher-secret: $REACHER_SECRET"

# only temporary/disposable addresses
curl "$REACHER_URL/v1/analytics/blocked?reason=disposable&limit=1000" \
  -H "x-reacher-secret: $REACHER_SECRET"
```

---

## 🤖 AI integration prompt

Paste the block below into your coding assistant (Cursor / Claude Code / Copilot),
fill in `{{STACK}}`, and provide `REACHER_URL` / `REACHER_SECRET` via env (get their
values from Railway — see above).

````text
You are integrating email validation into {{STACK}}.
Goal: at signup/input, reject temporary (disposable) and invalid email addresses,
showing a message like "We don't support this address".

=== SERVICE (self-hosted, forked Reacher) ===
- Base URL:  from env REACHER_URL
- Auth:      HTTP header  x-reacher-secret: <env REACHER_SECRET>
- Verify:    POST {REACHER_URL}/v1/check_email   body {"to_email":"<email>"}
  Response fields: is_reachable ("safe"|"risky"|"invalid"|"unknown"),
                   misc.is_disposable (bool), syntax.is_valid_syntax (bool)
- Analytics: GET  {REACHER_URL}/v1/analytics/blocked?from=&to=&reason=&limit=&offset=
  (blocked count + list; reason ∈ all|disposable|invalid|syntax)

=== HARD REQUIREMENTS ===
1. Call the service ONLY from the backend. The secret must never reach the browser,
   client bundle, or logs. Flow: frontend -> our backend /api/validate-email -> service.
   Return only { valid: boolean, reason?: string, message?: string } to the client.
2. Trigger on the frontend: validate on blur of the email field and again on submit.
   Debounce; do NOT call on every keystroke.
3. Normalize the email (trim + lowercase) before sending.
4. Call the service with a 5s timeout.
5. DECISION LOGIC (in order):
   a) syntax.is_valid_syntax == false        -> reject "Please enter a valid email"
   b) misc.is_disposable == true             -> reject "We don't support temporary email addresses"
   c) is_reachable == "invalid"              -> reject "This email address doesn't seem to exist"
   d) is_reachable == "safe" | "risky"       -> accept
   e) is_reachable == "unknown"              -> accept (fail-open): some providers can't be
                                                verified from a cloud IP; do NOT block them.
   f) timeout OR any service error           -> accept (fail-open) + log for monitoring.
      The checker being down must NOT block signups.

=== DELIVER ===
- Backend endpoint POST /api/validate-email  { email } -> { valid, reason?, message? }
- Reusable validateEmail(email) helper implementing the logic above
- Frontend wiring: inline error under the field, block submit when valid=false
- Config via env only: REACHER_URL, REACHER_SECRET (from a secret store, not committed)

Implement it in {{STACK}} following the project's conventions. Localize the messages.
````

---

## ⚙️ Operational notes

- **Analytics needs Postgres.** Set `RCH__STORAGE__POSTGRES__DB_URL` on the service
  (Railway → Postgres → `DATABASE_URL`). Reacher then records every verification into
  `v1_task_result`, and `/v1/analytics/blocked` reads from it.
- **No backfill.** Analytics only counts verifications made *after* Postgres was enabled.
- **Outbound port 25 & proxies.** SMTP verification needs outbound port 25. If it's
  blocked (or the egress IP has poor reputation), results come back as `unknown`. For
  reliable, high-volume verification, route SMTP through a SOCKS5 proxy
  (`RCH__PROXY__*`, e.g. [proxy25.com](https://proxy25.com)).
- **Disposable list.** `core/src/misc/disposable.txt` is embedded at compile time — after
  editing it (or the allowlist), rebuild and redeploy. Regenerate from a raw list with
  `make normalize-disposable RAW=path/to/list.txt`.

---

## 📄 License

Based on [`check-if-email-exists`](https://github.com/reacherhq/check-if-email-exists)
by Reacher, available under the [AGPL-3.0](./LICENSE.AGPL). This fork inherits the same
license. See [Reacher's licensing docs](https://docs.reacher.email/self-hosting/licensing)
for the upstream dual-license details.

## 🔨 Build from source

See [`backend/README.md`](./backend/README.md#build-from-source).
