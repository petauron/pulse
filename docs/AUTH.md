# Administrator authentication

Pulse keeps browser administrator sessions separate from Agent bearer credentials.
The dashboard's read APIs are private by default. Even if `private_site` is later
disabled by an administrator, no monitoring data is exposed before initial setup.
Administration APIs always require a complete administrator session.

## First setup

Set `PULSE_PUBLIC_URL` to the exact browser-facing origin, for example
`https://pulse.example.com`. It must be HTTPS except for HTTP loopback development.
Subpath deployments are not supported. Pulse uses this configured origin for
Origin checks, OAuth callbacks, and cookie security; forwarded headers do not
change these decisions.

Create a private file containing at least 32 bytes of cryptographically random
secret text and configure its path through `PULSE_SETUP_TOKEN_FILE`. Keep the file
readable only by the Service administrator, and mount it read-only in a container.
The token is not generated, printed, or placed in a URL by Pulse. Open the login
page, supply that token, and choose the administrator username and password.

The first setup runs in an immediate SQLite transaction and creates the only
administrator account. Concurrent or later setup attempts cannot replace it.
After setup, remove the token file and its environment configuration together.
There is no public registration or automatic administrator account creation.

Passwords must contain 12–1024 bytes. Usernames contain 1–64 ASCII letters,
digits, underscores, hyphens, or periods. Passwords use RustCrypto Argon2id PHC
hashes with independently generated salts. Authentication errors do not include
passwords or tokens.

## Sessions and request protection

Sessions expire after 12 hours without sliding renewal; at most eight session
records are retained. Session cookies are `HttpOnly`, `SameSite=Lax`, host-only,
and use `Path=/`. HTTPS uses the `__Host-` cookie prefix and `Secure`. Cookie
values contain two independently generated UUID v4 values; only their SHA-256
hashes are stored in SQLite.

Deploy exactly one Service process per SQLite database. Browser administration
requests hold a shared session lease; authentication mutations hold an exclusive
lease, acquired before authorization with a two-second queue deadline. OAuth's
external identity exchange does not hold the lease; only final session issuance
acquires it and rechecks the administrator's authentication version transactionally.
Before acquiring an exclusive lease, authentication mutations must upload
their entire request body within two seconds and an 8 KiB limit; a slow anonymous
upload cannot hold the session lease. Running database workers keep their lease
even if the HTTP request is cancelled or times out. A successful session revocation therefore waits for
previously admitted administration work to finish, and later requests must
authenticate again. This ordering is in-process, not a multi-instance database
coordination mechanism.

Every browser mutation, including setup, login, logout, and starting OAuth,
requires both an exact `Origin` match and `X-CSRF-Token`. Clients first obtain
the token from `GET /api/auth/status` and preserve its cookies. Anonymous status
creates a separate CSRF cookie; authenticated CSRF tokens are derived from the
session credential with a separate purpose string. Tokens are compared in
constant time. Refresh status after login, logout, or a session change.

Authentication responses use `Cache-Control: no-store` and
`Referrer-Policy: no-referrer`. Authentication attempts share a process-wide
limit of 20 per minute. The limiter retains at most 20 timestamps, works without
proxy headers, and is not durable across Service restarts. Password hashing and
authentication database work use one bounded worker, with a two-second queue
deadline. Argon2's default memory cost is approximately 19 MiB for that worker.

An administrator can change `private_site` to expose read-only monitoring data.
That setting never grants mutation access. Agent enrollment and snapshot ingestion
continue to use their own bearer authentication and do not accept browser cookies
as an Agent credential.

## Two-factor authentication

Enabling TOTP requires a valid administrator session and the current password.
Pulse returns a Base32 secret and an `otpauth://` URI. Store the secret securely
in an authenticator or password manager; it is not sent to a QR-code service.
The pending enrollment expires after ten minutes and is bound to the session
that started it. A valid six-digit code confirms enrollment.

TOTP uses a 30-second period and allows one period of clock drift. Pulse records
the last accepted step inside the authentication transaction and refuses replay.
Wait for the next code after login before changing sensitive account settings.
TOTP secrets must remain recoverable by the Service for verification and are
stored in its private SQLite database; protect the database and its backups as
secrets. Keep a securely backed-up authenticator secret: recovery codes and
self-service password/2FA recovery are not provided.

Password changes require the old password and a TOTP code when enabled. Disabling
TOTP requires both the password and a fresh code. Password changes and enabling
or disabling TOTP revoke every browser session, pending TOTP enrollment, and OAuth
flow, and increment the administrator credential generation. Log in again after
these operations.

## Optional GitHub OAuth

GitHub OAuth is disabled unless an administrator explicitly configures
`PULSE_GITHUB_CLIENT_ID`, `PULSE_GITHUB_CLIENT_SECRET_FILE`, and
`PULSE_GITHUB_ALLOWED_USER_ID` together. The last setting is the allowed account's
immutable numeric GitHub user ID.
Use a dedicated GitHub OAuth application with callback URL
`PULSE_PUBLIC_URL/api/auth/oauth/callback`. Client secrets must be supplied through
the deployment's secret configuration, never committed to Git.

Initial password setup remains required. OAuth authenticates only that existing
administrator, never creates accounts, and validates the numeric account ID on
every login. A mutable GitHub login name is not an access-control identifier.
The flow uses a ten-minute, single-use state record, browser binding cookie,
PKCE S256, exact callback URL, HTTPS, and no requested OAuth scopes. Pulse accesses
only GitHub's token endpoint and current-user identity API when OAuth is used.
It neither stores the GitHub access token nor requests repository access.

At most 32 pending OAuth records are retained. OAuth HTTP requests have bounded
timeouts, disable redirects, and read at most 64 KiB per response. If local TOTP is
enabled, the GitHub callback grants only a ten-minute incomplete session. That
session cannot read private data or administer Pulse until the local TOTP code is
verified. A completed login rotates the temporary session credential.

## API contract

`GET /api/auth/status` returns:

```json
{
  "initialized": true,
  "logged_in": true,
  "username": "admin",
  "csrf_token": "64-character token",
  "totp_enabled": false,
  "oauth_enabled": false,
  "oauth_totp_required": false
}
```

Anonymous `username` is `null`. `oauth_totp_required` identifies an incomplete
OAuth session. All POST calls preserve cookies and send `Origin` and
`X-CSRF-Token`; JSON calls use `Content-Type: application/json`.

| Endpoint | JSON request | Success |
| --- | --- | --- |
| `POST /api/auth/setup` | `{token, username, password}` | Status object and session cookie |
| `POST /api/auth/login` | `{username, password, code?}` | Status object and session cookie |
| `POST /api/auth/logout` | `{}` | `{ok:true}` and cleared session cookie |
| `POST /api/auth/password` | `{current_password, new_password, code?}` | `{ok:true}` and all sessions revoked |
| `POST /api/auth/totp/setup` | `{password}` | `{secret, otpauth_url, expires_in:600}` |
| `POST /api/auth/totp/enable` | `{code}` | `{ok:true}` and all sessions revoked |
| `POST /api/auth/totp/disable` | `{password, code}` | `{ok:true}` and all sessions revoked |
| `POST /api/auth/oauth/start` | `{}` | `{authorization_url}`; navigate the browser to it |
| `GET /api/auth/oauth/callback` | Query `code`, `state` | Redirect `/` or `/?oauth_totp=required` |
| `POST /api/auth/oauth/complete` | `{code}` | Status object and full session cookie |

Application errors return `{error: "message"}` with the appropriate HTTP error
status. Malformed JSON and oversized request bodies use Axum's standard extractor
errors. Authentication request bodies are limited to 8 KiB.

Authentication tables live in the main Service database and are installed only
by its backed-up, forward-only schema migration. Authentication startup opens an
existing migrated database and fails closed when its tables are absent.

`deploy/service.env.example` documents systemd settings. The initial-setup Compose
example is `deploy/docker-compose.yml`; it requires an explicitly selected image
built from this revision, an HTTPS public origin, and a private setup-token source
file. The older Alpha.2 release does not contain these authentication APIs. After
initialization, remove the Compose setup-token environment entry and secret mount
together before removing its source file. OAuth entries remain commented until
explicitly configured.

Implementation references: [RustCrypto Argon2](https://docs.rs/argon2/0.5.3/argon2/),
[totp-rs](https://github.com/constantoine/totp-rs), and
[GitHub OAuth authorization](https://docs.github.com/en/apps/oauth-apps/building-oauth-apps/authorizing-oauth-apps).
