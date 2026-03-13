# veNEAR Observatory Web

This app is the Next.js App Router frontend for the canonical veNEAR governance API.

## Local development

```bash
# Optional: override the default local API URL
cp .env.example .env.local
pnpm install
pnpm dev
```

The app resolves its API base URL in this order:

1. `VENEAR_API_BASE_URL`
2. `NEXT_PUBLIC_VENEAR_API_BASE_URL`
3. `http://127.0.0.1:3000/api/v1/venear`

Use `VENEAR_API_BASE_URL` for server-side deployments such as Railway private
networking. `NEXT_PUBLIC_VENEAR_API_BASE_URL` is available as a fallback when
you need to expose the base URL to browser-rendered code.

## Checks

```bash
pnpm lint
pnpm build
```

## Railway

The Railway service expects:

- `NEXT_TELEMETRY_DISABLED=1`
- `VENEAR_API_BASE_URL=http://${{api.RAILWAY_PRIVATE_DOMAIN}}:${{api.PORT}}/api/v1/venear`
- Optional client-visible fallback: `NEXT_PUBLIC_VENEAR_API_BASE_URL=http://${{api.RAILWAY_PRIVATE_DOMAIN}}:${{api.PORT}}/api/v1/venear`

The API service already reads Railway's injected `PORT` environment variable, so
you should not need to hardcode an internal port for service-to-service traffic.

The committed deployment manifest is `web/railway.toml` and the container build is `web/Dockerfile`.
