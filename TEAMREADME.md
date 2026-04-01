# Fidget

> One-to-many live streaming platform with multi-server CDN delivery

One-to-many live streaming video (like Twitch), tested with users in different locations (w/ a VPN), then establish multiple servers and orchestrate the delivery between those.

## Tech Stack

- **Frontend:** React (Vite)
- **Backend:** Rust / Axum
- **Database:** PostgreSQL
- **Media:** Nginx-RTMP, FFmpeg, HLS
- **Infrastructure:** Docker, Docker Compose
- **Cloud:** Oracle Cloud Free Tier or DigitalOcean (via GitHub Student Pack)

## Cloud Deployment

We're choosing between two providers:

| Provider | Credits | Pros | Cons |
|----------|---------|------|------|
| **Oracle Cloud Free Tier** | Always free (4 OCPU, 24 GB RAM ARM) | Most compute for $0, never expires | Less documentation, UI can be clunky |
| **DigitalOcean** | $200 via GitHub Student Developer Pack | Easiest setup, great docs, simple UI | Credits expire, less free compute |

**Decision:** TBD — team to evaluate both and pick by Sprint 2.

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
