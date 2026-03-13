# veNEAR Observatory Web

This app is the Next.js App Router frontend for the canonical veNEAR governance API.

## Local development

```bash
cp .env.example .env.local
pnpm install
pnpm dev
```

By default the app reads from `http://127.0.0.1:3000/api/v1/venear`. Override that with `VENEAR_API_BASE_URL` if the API is running elsewhere.

## Checks

```bash
pnpm lint
pnpm build
```

## Railway

The Railway service expects:

- `NEXT_TELEMETRY_DISABLED=1`
- `VENEAR_API_BASE_URL=http://${{api.RAILWAY_PRIVATE_DOMAIN}}:${{api.PORT}}/api/v1/venear`

The API service already reads Railway's injected `PORT` environment variable, so
you should not need to hardcode an internal port for service-to-service traffic.

The committed deployment manifest is `web/railway.toml` and the container build is `web/Dockerfile`.
