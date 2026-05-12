# Fidget
CHECKOUT
Dakota - END TO END streamer -> server -> viewer pipeline
BEN - stream key validation
Chandler - DEPLOY 
JOSH - NODE container


> One-to-many live streaming platform with multi-server CDN delivery

One-to-many live streaming video (like Twitch), tested with users in different locations (w/ a VPN), then establish multiple servers and orchestrate the delivery between those.

## Tech Stack

- **Frontend:** React (Vite), `hls.js`, native `WebTransport` + `WebRTC`
- **Backend:** Rust / Axum / SQLx, `wtransport` for QUIC, `bcrypt` + `jsonwebtoken` for auth
- **Database:** PostgreSQL 16
- **Media:** SRS 5 (RTMP + WebRTC bridge + HLS), FFmpeg (libx264 + AAC) for transcoding & rendition fan-out
- **Edge / CDN:** nginx (shared read-only HLS volume), opt-in CloudFront
- **Routing:** Rust + MaxMind GeoLite2
- **Infrastructure:** Docker Compose (with optional `turn` profile for coturn)
- **Cloud:** Oracle Cloud Free Tier or DigitalOcean (via GitHub Student Pack)

## Current State

| Area | Status | Notes |
|------|--------|-------|
| Core backend API (auth + streams) | Implemented | Axum routes for register/login, stream CRUD, join, chat, admin, role applications. |
| User roles + admin bootstrap | Implemented | `admin` / `streamer` / viewer roles. Admin row is upserted from `ADMIN_USERNAME` / `ADMIN_PASSWORD` on every backend boot. |
| Role applications | Implemented | Users can request `streamer`; admins approve/deny from `/admin`. |
| QUIC / WebTransport ingest | Implemented | Single ordered unidirectional stream; container auto-detect (WebM EBML / fMP4 ftyp). |
| Cert rotation | Implemented | Self-signed QUIC identity rotates every 10 days; cert hash served from a shared `Arc<RwLock<String>>`. |
| OBS / RTMP ingest | Implemented | SRS `on_publish` hook calls back to backend for stream-key validation. |
| Multi-rendition encoding | Implemented | ffmpeg `-filter_complex split=N` fans out to source / 480p / 360p RTMP outputs. |
| WHEP playback | Implemented | Frontend tries WHEP for ~3.5 s, falls back to HLS via `hls.js` / Safari native. |
| Real-time chat | Implemented | Carried over the same WebTransport endpoint as ingest; REST history under `/api/chat/`. |
| Edge HLS delivery | Implemented | Edge nginx mounts the `hls_segments` volume read-only; serves `/hls/<key>/index.m3u8`. |
| Geo router | Implemented | `EDGE_SERVERS` parsed list → `CLOUDFRONT_DOMAIN` → `http://edge:8090` fallback. |
| Monitoring + testing docs | Implemented | See [docs/testing.md](docs/testing.md) and `monitoring/` Prometheus + Grafana dashboards. |
| Cloud deployment | In progress | Dockerized stack is production-shaped; provider selection / multi-region rollout still being finalised. |

### Immediate Next Priorities

1. Multi-region deployment validation (CloudFront + multiple edge nginx instances).
2. LL-HLS / hardware-accelerated encode for lower glass-to-glass latency.
3. Automated load testing beyond `scripts/simulate_viewers.sh`.

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

### How the live feed is transmitted

Two ingest paths coexist, depending on what the streamer's browser supports:

- **Chrome / Edge:** the browser opens a **WebTransport (QUIC)** session to the Rust backend and writes WebM or fMP4 onto a single ordered unidirectional stream.
- **Firefox / Safari / dedicated software:** the streamer points OBS (or any RTMP encoder) directly at the SRS media server.

#### Why QUIC / WebTransport for browser ingest

The browser cannot speak RTMP, so the only way to ship video out of a tab
without WebRTC's signalling overhead is **WebTransport** (QUIC over UDP).
QUIC gives us TLS 1.3 by default, 0-1 RTT setup, no head-of-line blocking
between streams, and connection migration across IP changes — all
properties TCP can't match for live media.

#### Why an ordered stream instead of unreliable datagrams

The first prototype used QUIC unreliable datagrams (RFC 9221) with a
custom 8-byte fragmentation header. That worked in Chrome but failed in
practice for two reasons:

1. **Cross-browser flakiness.** Firefox in particular silently dropped
   datagrams on many network paths, even after the WebTransport session
   was established.
2. **Unnecessary complexity.** `MediaRecorder` produces a self-delimiting
   container byte stream (WebM clusters or fMP4 fragments). Splitting it
   into ~1,200-byte datagrams and reassembling them on the server
   re-implemented something QUIC's stream layer already does correctly.

A single ordered unidirectional stream removed the entire
fragmentation/reassembly path and made ingest reliable on every
WebTransport-capable browser. The trade-off — losing the "skip stale
frame" property of unreliable delivery — is a non-issue at our 100 ms
MediaRecorder timeslice and zero-latency encoder settings.

#### Self-signed cert + 10-day rotation

The QUIC server uses a self-signed TLS identity. The browser trusts it
via SHA-256 hash pinning (`serverCertificateHashes`) instead of a CA
chain. Chrome rejects pinned certs whose validity period exceeds **14
days**, and `wtransport::Identity::self_signed` defaults to exactly 14
days, so a long-lived backend silently broke browser publish after two
weeks of uptime (OBS / RTMP kept working, which made the bug look
intermittent).

The backend now **rotates the identity every 10 days**: `tokio::select!`
races the accept loop against a sleep timer, the endpoint is rebound,
and the new hash is published into a shared `Arc<RwLock<String>>` that
`GET /api/transport/cert-hash` reads on every request. The next "Go
Live" automatically pins the new cert.

```
  Browser publish  ── WebTransport unidirectional stream ─►  Backend QUIC  ── stdin ─►  ffmpeg  ── RTMP × N renditions ─►  SRS 5
  (Chrome / Edge)         (WebM / fMP4)                       (wtransport)               fan-out                            │
                                                                                                                            │  RTMP, HLS, WHEP
  OBS / ffmpeg push  ────────────────────────────  RTMP  ──────────────────────────────────────────────────────────────────►│
                                                                                                                            ▼
                                          HLS shared volume ─► edge nginx ─► CloudFront (opt-in) ─► HLS viewers (hls.js / Safari)
                                                                                                  ╲
                                                                                                   ─► WHEP viewers (WebRTC, UDP :8200)
```

1. **Browser publish.** `MediaRecorder` captures the composited canvas (screen + optional webcam overlay) and emits a self-delimiting container stream every 100 ms. The frontend opens a WebTransport session, pins the backend's self-signed cert via `serverCertificateHashes` (fetched from `GET /api/transport/cert-hash`), creates one unidirectional stream, and writes a 1-byte primer followed by the recorder output in order.

2. **OBS publish.** OBS connects to `rtmp://localhost:1935/live` with the user's stream key. SRS calls back to `POST /api/internal/rtmp/on_publish`; the backend looks up the key, checks the role, and answers `200` to accept or `403` to reject.

3. **Backend QUIC server.** `wtransport` accepts the session, validates the JWT, peeks the first ≤ 64 KB of the stream to detect WebM (EBML magic) vs fMP4 (`ftyp` box), and pipes everything into ffmpeg's stdin. The self-signed identity is rotated every 10 days and the new SHA-256 hash is published into a shared `Arc<RwLock<String>>` for subsequent cert-hash lookups.

4. **ffmpeg fan-out.** A single decode pass feeds `-filter_complex split=N` into N encoders that push to RTMP at `<key>`, `<key>_480p`, `<key>_360p`. fMP4 source can `-c:v copy` straight through.

5. **SRS 5.** Receives RTMP, writes HLS to `/tmp/hls/hls/<key>/` (300 s window, 0.5 s fragments), and bridges RTMP → WebRTC for WHEP playback over UDP :8200.

6. **Delivery.** Edge nginx mounts the `hls_segments` Docker volume read-only and serves `/hls/<key>/index.m3u8`. In production CloudFront fronts the edge (URL shape unchanged). The frontend nginx in this repo also reverse-proxies `/whep/` to SRS so WHEP signalling is same-origin.

7. **Viewer.** Tries WHEP for ~3.5 s; on timeout / failure falls back to HLS via `hls.js` (Chrome / Firefox / Edge) or native HLS (Safari).

### Encoding details

| Stage | Codec | Container / Transport | Notes |
|-------|-------|------------------------|-------|
| Browser → Backend | VP8 + Opus *or* H.264 + AAC | WebM or fMP4 over a QUIC unidirectional stream | Container auto-detected from the stream prefix; no fragmentation header. |
| Backend → SRS | H.264 + AAC | RTMP / FLV | ffmpeg `filter_complex split=N` fans out to source / 480p / 360p; fMP4 source can `-c:v copy`. |
| SRS → HLS viewers | H.264 + AAC | HLS (.m3u8 + .ts), 0.5 s fragments, 300 s window | One playlist per rendition under `/hls/<key>[_480p\|_360p]/`. |
| SRS → WHEP viewers | H.264 + Opus | WebRTC (DTLS-SRTP) over UDP :8200 | Same RTMP source bridged into the WebRTC pipeline. |

### ffmpeg flags (per rendition)

| Flag | Purpose |
|------|---------|
| `-preset veryfast` | Fast encoding to minimize latency |
| `-tune zerolatency` | Disables encoder look-ahead buffering |
| `-g 60` | Keyframe every 60 frames (2 s at 30 fps) for seek-friendly segments |
| `-b:v {1000k\|600k}` | Per-rendition video bitrate (480p / 360p) |
| `-b:a {128k\|96k\|64k}` | Per-rendition AAC audio bitrate |

### Browser support

| Capability                    | Chrome / Edge | Firefox          | Safari          |
|-------------------------------|---------------|------------------|-----------------|
| Browser publish (WebTransport)| Yes           | No (pref-gated)  | No              |
| OBS / RTMP publish            | Yes           | Yes              | Yes             |
| WHEP playback                 | Yes           | Yes              | Yes             |
| HLS playback                  | Yes           | Yes              | Yes (native)    |
| Chat (WebTransport)           | Yes           | No (pref-gated)  | No              |

### Future encoding plans

- **Hardware acceleration:** NVENC or VAAPI on the server for lower CPU during transcoding.
- **VP9 / AV1 pass-through:** when SRS supports it, skip the H.264 transcode for fMP4 sources that already match.
- **Low-latency HLS (LL-HLS):** partial segments + preload hints for sub-second HLS-fallback latency.
- **Adaptive bitrate (ABR):** automatic rendition selection in `hls.js` (today the viewer picks manually).
