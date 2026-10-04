# RustKeep

Self-hosted notes, files, API key vault, reminders and lightweight project boards.

- `./` - server: axum + libSQL (local SQLite file or Turso), single-threaded tokio runtime.
- `web/` - client: Leptos CSR (nightly) with [rust-ui](https://github.com/rust-ui/ui) components and Tailwind v4.

## Run with Docker

```bash
docker build -t rustkeep .
docker run -p 8080:8080 -v rustkeep-data:/data -e PUBLIC_URL=https://example.com rustkeep
```

## Local development

```bash
cd web && npm install && trunk build --release && cd ..
cargo run --release
```

## Configuration

Copy `.env.example` to `.env`. The server loads `.env` from the working directory on start; with Docker use `docker run --env-file .env ...`. Real environment variables override `.env`.

| Variable | Default | Purpose |
| --- | --- | --- |
| `BASE_PATH` | empty | Path prefix, e.g. `/rustkeep` |
| `PUBLIC_URL` | `http://localhost:8080` | External origin used in email links; `https://` enables secure cookies |
| `PORT` | `8080` | Listen port |
| `DATA_DIR` | `data` | Uploaded files, local database and generated `secret.key` |
| `STATIC_DIR` | `web/dist` | Built client |
| `TURSO_DATABASE_URL` | `$DATA_DIR/rustkeep.db` | `libsql://...` for Turso, otherwise a local file path |
| `TURSO_AUTH_TOKEN` | empty | Turso token |
| `SECRET_KEY` | generated file | Encryption key for vault entries and secret notes |
| `RESEND_API_KEY` | empty | Resend key; without it emails are printed to stdout |
| `MAIL_FROM` | `RustKeep <noreply@efeozkan.com.tr>` | Sender address (must be a verified Resend domain) |
| `VERIFY_EMAIL` | `example@efeozkan.com.tr` | Receives new-user approval requests |

Keep `SECRET_KEY` (or `DATA_DIR/secret.key`) safe: losing it makes vault entries and secret notes unreadable.
