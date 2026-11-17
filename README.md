# open-comms

Rust server with a cross-platform TypeScript web client. One web codebase runs in modern browsers on Windows, macOS, Linux, Android and iOS; the server serves the built web app and exposes the API/WebSocket endpoints.

## Current status
Initial implementation in progress:
- `crates/common`: shared protocol/domain types (channels, users/sessions, messages with embedded replies, reactions, WS events)
- `crates/server`: Axum server with HTTP APIs, WebSocket realtime events, MongoDB persistence/indexes, minimal auth/session baseline, and static hosting for the web build
- `apps/web`: Vite + TypeScript web client (sign-in, channels, messages, collapsible nested replies, emoji reactions, realtime updates)
- `apps/desktop`: Tauri v2 WebView2 shell for Windows local testing; embeds `apps/web/dist` and talks to `http://localhost:8080`
- `docker-compose.yml`: server + MongoDB deployment; the server image builds the web client and serves it from `/`
- `deploy/dev-https`: opt-in, dev-only HTTPS reverse proxy with a generated self-signed CA for browser capture testing

## Decisions captured
- No guild/workspace support (single-server channel model)
- Permissions fully permissive for now (authorization hook points added for later)
- No end-to-end encryption; in-transit encryption (TLS/WSS, WebRTC DTLS-SRTP) for MITM protection
- WebRTC with server-side relay/SFU behavior for voice/screen/window/audio sharing
- Web-first client: one TypeScript web app shared across all platforms
- Mobile targets (Android/iOS) are first-class with best-effort feature parity; browser/OS capture limitations (e.g. iOS screen/system-audio capture) are handled as close to desktop parity as possible

## Run locally
1. Start MongoDB (or use Docker for mongo only).
2. Set environment variables (see `.env.example`).
3. Build the web client:
   ```bash
   cd apps/web
   npm ci
   npm run dev
   ```
   Vite proxies API/WS requests to `http://localhost:8080` during development.
4. In another terminal, run the server:
   ```bash
   cargo run -p open-comms-server
   ```
   Health check: `GET http://localhost:8080/health`
   Voice uses UDP `3478` by default; allow it through your local firewall. Browsers ignore loopback ICE candidates, so set `MEDIA_ADVERTISE_ADDR` to a non-loopback IP clients can reach before joining voice (see below).

For a production-style local build:
```bash
cd apps/web && npm ci && npm run build && cd ../..
cargo run -p open-comms-server
```
The server serves `apps/web/dist` by default. Override with `WEB_DIST_DIR`.

## Run with Docker Compose
```bash
docker compose up --build
```
This starts MongoDB and the server on port `8080`, plus UDP port `3478` for WebRTC media. The server image builds `apps/web` and serves it at `http://localhost:8080`.

For **any browser voice session**, including one on the same machine, set `MEDIA_ADVERTISE_ADDR` to a non-loopback IP that the client can reach (for example, your LAN IP `192.168.x.x:3478` or a public IP). Browsers ignore `127.0.0.1` ICE candidates, so the default will connect signaling but never establish media. On Windows Docker, use the host LAN IP, not `127.0.0.1` or the Docker gateway address. Browser WebRTC also requires a secure context (`https://` or `localhost`); use a TLS reverse proxy for remote deployments.

### Browser capture requires HTTPS

Browsers only expose `getUserMedia` (microphone) and `getDisplayMedia` (screen/window) in a **secure context**. `http://localhost:8080` and the Tauri shell's `http://tauri.localhost/` are secure contexts. Plain HTTP on a LAN IP or public hostname (for example, `http://192.168.1.10:8080`) is not, so capture APIs are missing and voice/screen sharing cannot start. The client now reports this explicitly instead of claiming the browser is unsupported.

For browsers on other devices, put the server behind an HTTPS reverse proxy using a trusted certificate. The web client automatically switches WebSocket to `wss://` when the page is loaded over `https://`; no client rebuild is needed when proxying the same origin.

#### Dev-only self-signed HTTPS (Docker Compose)

An opt-in Compose profile starts an nginx reverse proxy that generates a local CA and a leaf certificate with SANs for `localhost`, `127.0.0.1`, and every `HTTPS_HOSTS` entry. This is for development only; production should use a certificate from a trusted CA.

Windows (PowerShell):

```powershell
$env:HTTPS_HOSTS = 'localhost,192.168.2.124'
$env:MEDIA_ADVERTISE_ADDR = '192.168.2.124:3478'
docker compose --profile https up --build -d
```

Linux/macOS:

```bash
HTTPS_HOSTS=localhost,192.168.2.124 MEDIA_ADVERTISE_ADDR=192.168.2.124:3478 \
  docker compose --profile https up --build -d
```

Open `https://192.168.2.124:8443` (or `https://localhost:8443`). Set `HTTPS_PORT` in `.env` or the environment to use a host port other than `8443`. Generated files live in `deploy/dev-https/certs/` and are gitignored.

Trust `deploy/dev-https/certs/ca.crt` once on each client device; until then browsers show a certificate warning and capture remains unavailable.

Windows (PowerShell):

```powershell
certutil -user -addstore Root .\deploy\dev-https\certs\ca.crt
```

macOS:

```bash
sudo security add-trusted-cert -d -r trustRoot \
  -k /Library/Keychains/System.keychain deploy/dev-https/certs/ca.crt
```

Linux:

```bash
sudo cp deploy/dev-https/certs/ca.crt /usr/local/share/ca-certificates/open-comms-dev.crt
sudo update-ca-certificates
```

Firefox uses its own certificate store; import the same CA under Settings → Privacy & Security → View Certificates → Authorities. Restart the browser after trusting the CA.

The CA is reused across restarts. If `HTTPS_HOSTS` changes, only the leaf certificate is regenerated, so the trusted CA does not need to be imported again. Use hostnames that resolve to this machine and/or IPv4 addresses; IPv6 literal SANs are not generated.

Example Caddyfile (Caddy on the same host as the published port):

```caddyfile
chat.example.com {
    reverse_proxy 127.0.0.1:8080
}
```

Example nginx:

```nginx
server {
    listen 443 ssl;
    server_name chat.example.com;
    ssl_certificate     /etc/letsencrypt/live/chat.example.com/fullchain.pem;
    ssl_certificate_key /etc/letsencrypt/live/chat.example.com/privkey.pem;

    location / {
        proxy_pass http://127.0.0.1:8080;
        proxy_http_version 1.1;
        proxy_set_header Upgrade $http_upgrade;
        proxy_set_header Connection "upgrade";
        proxy_set_header Host $host;
    }
}
```

If the proxy runs in Docker alongside the server, point it at `http://server:8080` instead of `127.0.0.1:8080`. The proxy only carries HTTP/WebSocket; WebRTC media still uses UDP `3478`, so keep that port reachable and `MEDIA_ADVERTISE_ADDR` set to a client-reachable address.

## Windows local testing app
`apps/desktop` is a small Tauri WebView2 shell around the existing web client. It does not duplicate any UI code: the Rust shell embeds `apps/web/dist` and the shared web code targets `http://localhost:8080` for HTTP/WebSocket when running inside Tauri.

Prerequisites on Windows:
- Windows 10/11 with the Edge WebView2 runtime (included on modern Windows)
- Microsoft C++ Build Tools with the **Desktop development with C++** workload
- Rust with the MSVC toolchain (`rustup default stable-x86_64-pc-windows-msvc`)
- Node.js and npm

1. Set `MEDIA_ADVERTISE_ADDR` to your non-loopback LAN IP (for example, `$env:MEDIA_ADVERTISE_ADDR='192.168.1.10:3478'` in PowerShell), then start the server (for example, `docker compose up --build`) and leave it running. This is required for voice even when the Tauri app is on the same machine.
2. Open a terminal in `apps/desktop` and install the Tauri CLI:
   ```powershell
   npm install
   ```
3. Launch the app in development mode:
   ```powershell
   npm run dev
   ```
   This starts the Vite dev server automatically and opens a window titled **Open Comms**.
4. To produce a standalone executable instead:
   ```powershell
   npm run build
   ```
   The exe is written to `apps/desktop/src-tauri/target/release/open-comms-desktop.exe`.

The shell intentionally has bundling disabled (`bundle.active = false` in `apps/desktop/src-tauri/tauri.conf.json`); `npm run build` uses `--no-bundle` so no installer/MSI tooling is needed. If the backend is not on `localhost:8080`, set `VITE_API_ORIGIN` when building the web assets and update the `connect-src` CSP in `tauri.conf.json`.

## API (current baseline)
### Auth / identity
- `POST /auth/register` `{ "username": "alice" }` → `{ "user": {...}, "token": "..." }` (creates user if new, issues session token)
- `GET /auth/me` with `Authorization: Bearer <token>` → current user or `null`

Identity is **permissive for now**:
- Authenticated requests use the session user.
- For compatibility, write actions may still accept `author_id`/`user_id` in the body if no session is provided.

### Channels & text
- `GET /channels`, `POST /channels` (`{ "name": "general", "channel_type": "text"|"voice" }`)
- `GET /channels/:channel_id/messages?limit=50`
- `POST /channels/:channel_id/messages` (`{ "body": "...", "author_id?": "<uuid>" }`)
- `POST /channels/:channel_id/messages/:parent_id/replies` (`{ "body": "...", "author_id?": "<uuid>" }`) — replies stored inside the root message (nested/repliable)
- `POST /channels/:channel_id/messages/:message_id/reactions` (`{ "emoji": "👍", "user_id?": "<uuid>" }`) — toggles reaction

### WebSocket
- `GET /ws?token=<session_token>` (token optional)
- Client events: `ping`, `send_message`, `reply_message`, `add_reaction`, `join_voice`, `rtc_answer`, `rtc_offer`, `ice_candidate`, `leave_voice`
- Server events: `pong`, `channel_created`, `message_created`, `reply_created`, `reaction_updated`, `voice_joined`, `voice_left`, `rtc_offer`, `rtc_answer`, `ice_candidate`, `voice_error`, `error`

## Web client capabilities
- Sign in with username (session token stored in `localStorage`)
- Channel list and channel creation (text/voice)
- Text messages with realtime updates
- Collapsible replies stored inside the parent message; replies can be replied to recursively
- Emoji reactions with toggle behavior
- Voice channels with server-relayed WebRTC audio, mute toggle, and per-participant remote media playback
- Screen/window sharing (with best-effort system/tab audio) using `getDisplayMedia`, with a local preview
- Responsive layout for desktop and mobile browsers

Browser limitations: `getDisplayMedia` cannot capture a specific "window" on all platforms, and system audio capture is not available on every browser/OS (notably many Linux and mobile browsers). The client falls back to video-only sharing when audio capture fails.

## Next implementation steps
- Refine WS semantics (per-channel subscriptions, request/ack correlation)
- Cursor-based history pagination and embedded-reply size/depth limits
- Harden media deployment (TURN for restrictive NATs, production TLS/WSS with trusted certificates, port range guidance)
- Validation/rate limiting and expanded tests/CI
- Package a signed Windows installer when the local-testing shell is ready for distribution
