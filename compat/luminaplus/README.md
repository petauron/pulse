# LuminaPlus v1.3.5 for Pulse

The installable `LuminaPlus-v1.3.5-pulse.zip` is derived from
[Komari-Theme-LuminaPlus v1.3.5](https://github.com/shanyang242/Komari-Theme-LuminaPlus/releases/tag/v1.3.5),
upstream commit `a52305e1c26c68bd741a079c4ff50f7b3a5a016e`.

SHA-256: `040ee3f409551065b50e55e7e65db27be31795bb62e540a6eb30748339034c6b`.

`pulse.patch` adds the Pulse CSRF header to settings saves and IP refreshes, and a build-time
HTTP RPC transport option. It accepts null optional hardware metrics without
discarding load/traffic history, using LuminaPlus's existing zero fallback for
unavailable metrics. The router dependency is updated to 7.18.4; production
dependency auditing reports no known vulnerabilities at validation time.
The visual components are unchanged. The upstream
MIT license is included here and inside the ZIP at `dist/LICENSE.LuminaPlus`.

Rebuild from the exact upstream source:

```sh
git apply /path/to/pulse.patch
npm ci
npm run lint
npm test
VITE_PULSE_HTTP_RPC=true npm run build
cp LICENSE dist/LICENSE.LuminaPlus
node scripts/package-zip.mjs
```

Keep the upstream preview.png. Install the resulting ZIP in Pulse's
`/admin?section=themes`, activate it, and use `/?view=theme-manage` for theme
settings after signing in. Default Frankfurter requests originate in the
visitor's browser; a custom HTTPS rate provider can be set in theme settings.
See `docs/THEMES.md` for the API boundary and opt-in built-in IP information setup.
