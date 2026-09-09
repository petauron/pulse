# Pulse Web

Pulse Web is the embedded monitoring dashboard. Its interface is derived directly from Komari Theme Emerald v1.0.11, while its data layer is connected to Pulse Service.

```bash
npm ci
npm run verify:licenses
npm run lint
npm run dev
npm run build
npm run test:e2e
```

Development requests under `/api` are proxied to `http://127.0.0.1:8080`. Production assets are generated in `dist` and embedded into `pulse-service`.

The build registers only referenced Iconify glyphs and packages flags locally. Runtime API, image, script, style, and WebSocket requests are restricted to the Pulse origin by the Service content security policy.

See [UPSTREAM.md](UPSTREAM.md), [THIRD_PARTY_NOTICES.md](THIRD_PARTY_NOTICES.md), and the bundled `LICENSE.*` files for third-party attribution.
