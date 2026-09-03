# Security Policy

## Supported versions

Pulse is currently pre-alpha. Until the first release, security fixes are made on the default branch only. Supported release lines will be listed here once releases begin.

## Reporting a vulnerability

Do not open a public Issue for a suspected vulnerability.

Use [GitHub private vulnerability reporting](https://github.com/petauron/pulse/security/advisories/new). Include affected versions or commits, reproduction conditions, impact, and any suggested mitigation. Do not include real credentials or production data.

Maintainers will acknowledge a complete report when it is reviewed, coordinate remediation and disclosure, and credit reporters who want public attribution.

## Security boundaries

- Agents initiate outbound connections to a configured Service.
- Pulse does not provide arbitrary remote command execution.
- Credentials and enrollment tokens must never be logged or returned after their intended one-time display.
- Pulse does not send telemetry or crash reports by default.
