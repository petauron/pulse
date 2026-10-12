# Security Policy

## Supported versions

Senulun is currently alpha and has no supported release line yet. Security fixes are prepared on review branches and enter protected `main` only through required checks. Supported release lines will be listed here once releases begin.

## Reporting a vulnerability

Do not open a public Issue for a suspected vulnerability.

Use [GitHub private vulnerability reporting](https://github.com/petauron/senulun/security/advisories/new). Include affected versions or commits, reproduction conditions, impact, and any suggested mitigation. Do not include real credentials or production data.

Maintainers will acknowledge a complete report when it is reviewed, coordinate remediation and disclosure, and credit reporters who want public attribution.

## Development dependency policy

The formatter dependency `oxfmt` pins `tinypool` 2.1.0. The Web package uses a
parent-scoped npm override to select 2.1.2, addressing
[GHSA-5gmw-xhrv-c9v3](https://github.com/advisories/GHSA-5gmw-xhrv-c9v3) and
[GHSA-85c8-ppgw-ccpr](https://github.com/advisories/GHSA-85c8-ppgw-ccpr).
Keep the override and lockfile consistent when updating the formatter chain;
do not restore a vulnerable version or bypass dependency review. This package
is development tooling, not a dependency of the Service or Agent runtime.

## Security boundaries

- Agents initiate outbound connections to a configured Service.
- Senulun does not provide arbitrary remote command execution.
- Credentials and enrollment tokens must never be logged or returned after their intended one-time display.
- Remote Agents require HTTPS; unencrypted HTTP is accepted only on loopback for development.
- Public dashboard reads and authenticated Agent ingestion are separate surfaces.
- Dashboard APIs expose node names, platform details, and health metrics to anyone who can reach the Service; operators must add access control when that data is private.
- Senulun does not send telemetry or crash reports by default.
