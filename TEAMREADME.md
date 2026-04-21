# Fidget
CHECKOUT
Dakota - END TO END streamer -> server -> viewer pipeline
BEN - stream key validation
Chandler - DEPLOY 
JOSH - NODE container


> One-to-many live streaming platform with multi-server CDN delivery

One-to-many live streaming video (like Twitch), tested with users in different locations (w/ a VPN), then establish multiple servers and orchestrate the delivery between those.

## Tech Stack

- **Frontend:** React (Vite)
- **Backend:** Rust / Axum
- **Database:** PostgreSQL
- **Media:** Nginx-RTMP, FFmpeg, HLS
- **Infrastructure:** Docker, Docker Compose
- **Cloud:** Oracle Cloud Free Tier or DigitalOcean (via GitHub Student Pack)

## Current State (April 21, 2026)

| Area | Status | Notes |
|------|--------|-------|
| Core backend API (auth + streams) | Implemented | Axum routes for register/login and stream CRUD are wired and running in Docker. |
| QUIC ingest path | Implemented | Browser WebTransport -> backend QUIC server -> ffmpeg pipe -> RTMP publish flow is documented and integrated. |
| Frontend app pages | Implemented | Home, login/register, dashboard, and watch page exist and connect to backend APIs. |
| Router service | Implemented (baseline) | GeoIP-based nearest-edge selection is implemented with fallback when GeoLite2 DB is missing. |
| Media server auth hooks | Not implemented | `media-server/scripts/on_publish.sh` is still TODO, and nginx `on_publish`/`on_publish_done` callbacks are not enabled yet. |
| Edge caching layer | Placeholder | `edge/nginx.conf` currently returns a readiness response and does not proxy/cache HLS yet. |
| Monitoring + testing docs | Partial | Architecture docs are present; `docs/testing.md` is still a placeholder. |
| Cloud deployment | In progress | Dockerized stack is ready locally; provider selection/deployment details are still being finalized. |

### Immediate Next Priorities

1. Implement and wire stream-key validation in media-server `on_publish` callbacks.
2. Replace edge placeholder config with HLS proxy + cache policy.
3. Add monitoring metrics/dashboard and complete testing documentation.

## GeoIP-Based Viewer Routing

The CDN router uses MaxMind's GeoLite2 City database to perform location-aware edge server selection. When a viewer requests a stream, the router:

1. Looks up the viewer's IP address in the GeoLite2 database to determine their geographic coordinates
2. Computes the Haversine (great-circle) distance from the viewer to each registered edge server
3. Returns the URL of the nearest edge server

Edge servers are configured via the `EDGE_SERVERS` environment variable in the format `id,url,lat,lon;id,url,lat,lon;...`. If the GeoIP database is unavailable, the router falls back to returning the first configured edge.

### GeoLite2 Database Setup

1. Create a free [MaxMind account](https://www.maxmind.com/en/geolite2/signup)
2. Download the GeoLite2 City database (`.mmdb` format)
3. Place the file at `router/GeoLite2-City.mmdb`

> **Note:** Without the GeoLite2 database, the router still works — it just skips location-based selection and falls back to the first configured edge server.

## Cloud Deployment

We're choosing between two providers:

| Provider | Credits | Pros | Cons |
|----------|---------|------|------|
| **Oracle Cloud Free Tier** | Always free (4 OCPU, 24 GB RAM ARM) | Most compute for $0, never expires | Less documentation, UI can be clunky |
| **DigitalOcean** | $200 via GitHub Student Developer Pack | Easiest setup, great docs, simple UI | Credits expire, less free compute |

**Decision:** In progress — team is evaluating both and will finalize provider selection before full multi-region deployment.

## Setup

```bash
cp .env.example .env
docker-compose up --build
```

That's it — all six services start together. No separate `npm install` or `cargo build` needed.

| Service | URL |
|---------|-----|
| Frontend | http://localhost:3000 |
| Backend API | http://localhost:8080 |
| Media Server (RTMP) | rtmp://localhost:1935 |
| Media Server (HLS) | http://localhost:8088 |
| Edge Server | http://localhost:8090 |
| Router | http://localhost:8070 |

## Running After Setup

### Full stack (production-like)

```bash
docker compose up --build
```

Open http://localhost:3000 in a browser. The frontend proxies `/api` to the backend and `/hls` to the media server automatically.

### Frontend hot-reload (development)

If you're only working on the frontend and want instant hot-reload:

```bash
# Terminal 1 — start the backend services
docker compose up --build db backend media-server

# Terminal 2 — start the Vite dev server
cd frontend
npm install   # only needed the first time, or after package.json changes
npm run dev
```

The Vite dev server runs at **http://localhost:5173** and proxies API requests to the backend container.

> **Note:** The Vite proxy in `vite.config.js` targets `http://backend:8080` (the Docker hostname). If you run the backend _outside_ Docker, change it to `http://localhost:8080`.

### Useful commands

| Command | What it does |
|---------|--------------|
| `docker compose up --build` | Build & start everything |
| `docker compose up -d` | Start in background (detached) |
| `docker compose down` | Stop all services |
| `docker compose down -v` | Stop & **delete database volume** (fresh start) |
| `docker compose logs -f backend` | Tail backend logs |
| `docker compose logs -f frontend` | Tail frontend Nginx logs |

## Live Video Ingest Architecture

### How the live feed is transmitted (browser → server)

The browser cannot send RTMP directly, so we use **QUIC unreliable datagrams** via the WebTransport API to get video to the backend with minimal latency. Unlike TCP/WebSocket, QUIC datagrams don't suffer from head-of-line blocking — lost frames are simply skipped instead of delaying all subsequent frames.

See [docs/quic-transport.md](docs/quic-transport.md) for a deep dive on why QUIC datagrams and the full protocol design.

```
┌──────────┐  QUIC Datagrams (UDP)  ┌─────────────┐  stdin pipe  ┌────────┐  RTMP   ┌──────────────┐
│  Browser  │ ────────────────────► │ Rust Backend │ ──────────► │ ffmpeg │ ──────► │ nginx-rtmp   │
│ (MediaRec)│  fragmented WebM      │ (wtransport) │  reassembled │        │  FLV    │ media-server │
└──────────┘                        └─────────────┘   chunks     └────────┘         └──────┬───────┘
                                                                                           │ HLS
                                                                                      ┌────▼────┐
                                                                                      │ Viewers │
                                                                                      └─────────┘
```

1. **Browser (ScreenCapture component):** Uses the `MediaRecorder` API to capture the composited canvas stream (screen capture + optional webcam overlay) as **WebM with VP8 video and Opus audio**. Every 250 ms, a chunk is produced, split into ~1,200-byte fragments with an 8-byte header (`[chunk_id:u32][frag_index:u16][frag_total:u16]`), and sent as QUIC datagrams via WebTransport.

2. **Rust Backend (QUIC server via wtransport):** Listens on UDP port 4443. The browser opens a WebTransport session to `https://host:4443/ingest/<stream_id>?token=<jwt>`. The server validates the JWT, confirms stream ownership, then receives datagrams, reassembles fragments back into complete chunks, and pipes them to ffmpeg's stdin.

3. **Certificate trust:** The QUIC server uses a self-signed TLS certificate. At startup, the backend computes the SHA-256 hash of the leaf certificate and serves it at `GET /api/transport/cert-hash`. The browser fetches this hash and passes it to the WebTransport constructor via `serverCertificateHashes`, allowing it to trust the self-signed cert without a CA.

4. **ffmpeg (transcoder):** Reads reassembled WebM chunks from stdin and transcodes to **H.264 video + AAC audio**, then pushes the result as an RTMP/FLV stream to the nginx-rtmp media server at `rtmp://media-server:1935/live/{stream_key}`.

5. **nginx-rtmp (media server):** Receives the RTMP stream and segments it into **HLS** (.m3u8 + .ts files) with 3-second fragments, served over HTTP on port 8088.

6. **Viewers:** Consume the HLS playlist from the edge server / media server.

### Encoding details

| Stage | Codec | Container/Transport | Notes |
|-------|-------|---------------------|-------|
| Browser → Backend | VP8 video, Opus audio | WebM over QUIC datagrams | MediaRecorder output fragmented into ~1,200-byte QUIC datagrams |
| Backend → Media Server | H.264 video, AAC audio | FLV (over RTMP) | ffmpeg transcodes because RTMP/FLV requires H.264/AAC |
| Media Server → Viewers | H.264 video, AAC audio | HLS (.ts segments) | nginx-rtmp segments the RTMP feed into 3-second HLS chunks |

### ffmpeg flags

| Flag | Purpose |
|------|---------|
| `-preset veryfast` | Fast encoding to minimize latency |
| `-tune zerolatency` | Disables encoder look-ahead buffering |
| `-g 60` | Keyframe every 60 frames (2 s at 30 fps) for seek-friendly segments |
| `-b:a 128k` | AAC audio bitrate |

### Future encoding plans

- **Adaptive bitrate (ABR):** Generate multiple HLS renditions (e.g., 1080p, 720p, 480p) by running ffmpeg with multiple output maps so viewers on slower connections can downgrade automatically.
- **Hardware acceleration:** Use NVENC or VAAPI on the server for lower CPU usage during transcoding.
- **VP9/AV1 pass-through:** If the media server supports it in the future, skip the H.264 transcode and pass VP8/VP9 directly to reduce server load.
- **Low-latency HLS (LL-HLS):** Reduce segment size and use partial segments + preload hints for sub-second latency.
