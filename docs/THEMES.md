# Theme packages and Komari read compatibility

Pulse keeps Emerald embedded as the default. Administrators can install a trusted
Komari-style ZIP from Admin → Themes. The ZIP must have the manifest file
komari-theme.json and dist/index.html at its root. Pulse accepts up to
16 MiB compressed, 32 MiB expanded, 512 entries and 4 MiB per file. Paths,
links, duplicate names and special files are rejected. A package is not
activated on installation; activation is a separate administrator action.
Returning to Emerald does not remove installed packages.

The administrator UI and login page always use the built-in Emerald bundle.
Installed theme assets serve the public dashboard on the same origin. Theme
JavaScript is trusted code, not a sandbox: it can make credentialed requests
as the signed-in browser. Install only audited packages from trusted publishers.
Pulse does not automatically fetch a theme market or arbitrary remote URLs.
LuminaPlus is explicitly permitted to use its default HTTPS Frankfurter
exchange-rate API from the visitor's browser. Other custom HTTPS rate/image/video
origins are added narrowly to the theme's Content Security Policy after an
administrator saves the URLs. Light/dark background URL pairs are supported.
Reload the page after changing resource origins so the new policy takes effect.

Packages, the active-theme selector and per-theme JSON settings live in
the persistent directory beside PULSE_DATABASE_PATH, under themes/.
Back up this directory in addition to the two SQLite files. Restoring only
the database pair does not restore installed themes. If an active theme is
missing on startup, Pulse displays the embedded Emerald theme.

The following administrator routes require a Pulse session, exact Origin and
CSRF token for mutations:

- GET /api/admin/theme/list
- POST /api/admin/theme/install with an application/zip body
- POST /api/admin/theme/set with a JSON object containing theme
- POST /api/admin/theme/settings?theme=SHORT with a JSON settings object

The list and mutation responses use the Komari standard response envelope.
For installed themes, /api/public returns the selected theme identifier
and theme_settings, merging manifest defaults with saved settings.

## Monitoring API compatibility

Pulse implements a bounded, read-only subset commonly used by Komari themes:

- GET /api/public, /api/version, /api/me, /api/nodes
- GET /api/records/load?uuid=...&hours=...
- GET /api/records/ping?uuid=...&hours=...
- GET /api/task/ping
- POST /api/rpc2: rpc.ping, rpc.version, rpc.methods,
  common:getNodes, common:getNodesLatestStatus,
  common:getPublicInfo, common:getVersion,
  common:getNodeRecentStatus, common:getRecords,
  public:getNodesInformation, public:getPublicSettings,
  public:getVersion, public:getPublicPingTasks

The existing Pulse common:getDashboard remains available for Emerald.
Pulse enforces its private-site policy on all monitoring reads. Unknown RPC
methods return method-not-found. This is not full Komari API compatibility:
Pulse intentionally does not implement Komari Agent, remote execution, terminal,
file management or market APIs. Internal probe IDs remain Pulse UUIDs; the
Komari-facing task/history APIs expose stable JavaScript-safe numeric IDs.
GET /api/admin/ping and /api/admin/client/list supply settings selectors.
GET /api/admin/plugin/list returns an empty list because Pulse has no Komari
plugins. /api/recent/:uuid provides bounded flat Pulse
records as a fallback, not Komari's nested live snapshot shape. Theme-specific
features that require the omitted interfaces will need adaptation.

## LuminaPlus v1.3.5

Install compat/luminaplus/LuminaPlus-v1.3.5-pulse.zip through Admin → Themes,
then activate it. Installation alone never activates a package. The theme's
settings page is /?view=theme-manage after signing in. Pulse administration
remains at /admin, and LuminaPlus's /admin/ping link redirects to Pulse probes.

This package preserves the upstream visual implementation. It obtains a Pulse
CSRF token for settings saves and IP information refreshes, and uses the supported HTTP RPC transport instead
of repeatedly attempting a WebSocket handshake. The adapted source patch,
build instructions and upstream MIT license are beside the ZIP. The ZIP also
contains the license. Unmodified upstream packages can display monitoring
data, but do not include the Pulse settings-save adaptation.
The adapter also accepts null optional hardware metrics so valid load and
traffic samples are retained. Missing optional metrics use LuminaPlus's own
zero fallback; the native Pulse APIs continue to preserve null values.

Dashboard, node details/history, Ping binding, asset calculation, responsive
views and appearance/settings use Pulse's actual monitoring data. The optional
built-in IP information adapter described below supports LuminaPlus without a
Komari plugin runtime. Pulse does not fabricate this information. Newer public metric RPC methods
are not implemented: LuminaPlus uses its upstream records fallback, subject
to Pulse's retention and history limits. This is theme compatibility, not
general Komari administrative API compatibility.

## Optional built-in IP information

In `/admin?section=site`, explicitly enable **IP information (administrators only)**.
This is disabled by default, including existing installations. In the existing
node editor, register the node's public IPv4 and/or IPv6 address. Blank removes
an address. These fields are administrator-maintained metadata, not inferred
from proxies or collected by the Agent. No database schema migration is needed.
The admin node metadata API accepts `ipv4` and `ipv6`; site settings accept
`ip_info_enabled`. Registered IPs are returned to authenticated administrators
only, and only when the feature is enabled. They are absent from public node
and RPC responses. LuminaPlus retains its original IP information interface.

The built-in adapter implements:

- `GET /api/public/ip-info/v1/status`
- `GET /api/public/ip-info/v1/lookup?uuid=...&ip=...`
- `GET /api/public/ip-info/v1/latency?uuid=...&ip=...`
- `GET /api/admin/ip-info/v1/status`
- `POST /api/admin/ip-info/v1/refresh` with `uuid`, `ip`, and `include_latency`

Despite the compatibility `/public/` path, **all endpoints require an administrator**.
Refresh additionally requires same-origin and Pulse CSRF protection. Anonymous
requests are rejected before provider access, including on public sites.
Only literal, registered public IPs of non-revoked nodes can be queried; local,
private, multicast, reserved, documentation, mapped IPv4 and transition IPv6
addresses are rejected. China-mainland nodes are excluded before querying when
their configured region is CN; a CN provider result prevents global latency.
HK, MO and TW are not excluded. Node/address binding is checked even on cache hits.

**Privacy:** enabling this feature permits the Service to send the selected
registered IP to `https://ip.net.coffee` for location, ASN/network ownership,
native/broadcast/anycast classification, and six-location global latency.
Pulse credentials and host metrics are never sent. Requests originate from the
Service, not the browser. This implementation deliberately has no automatic
fallback to additional third parties. Third-party accuracy, availability and
terms are outside Pulse's control. Unknown fields remain null; no risk scores,
media-unlock results or AI-unlock results are invented.

Bounds per Service process: one serialized provider lane; 256 cached records;
256 KiB provider response limit; 6 provider calls/minute and 200/day (UTC);
24-hour lookup and 1-hour latency TTL; up to one additional day of stale data
with explicit warnings; five-minute cooldown after failure. Refresh does not
bypass quotas. Redirects are disabled, destinations are fixed HTTPS origins,
and proxy environment variables are not used. Lookup timeout is 6 seconds,
latency timeout 20 seconds, total route timeout 30 seconds. Cache and quota
counters are in memory and reset on restart; they are not an account-level
billing limit across replicas or restarts. Disabling prevents new queries but
does not retroactively cancel an already-dispatched provider request.

Protocol reference: [Komari-IP-Info](https://github.com/shanyang242/Komari-IP-Info).
This is a Rust implementation of the theme-facing API, not a Komari plugin
installer. The updated Pulse theme ZIP is required for CSRF-protected refresh.
For an already-installed package, export its settings before replacing it;
the installer does not silently overwrite an installed theme.
