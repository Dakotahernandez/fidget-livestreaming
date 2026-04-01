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

| Service | URL |
|---------|-----|
| Frontend | http://localhost:3000 |
| Backend API | http://localhost:8080 |
| Media Server (RTMP) | rtmp://localhost:1935 |
| Media Server (HLS) | http://localhost:8088 |
| Edge Server | http://localhost:8090 |
| Router | http://localhost:8070 |
