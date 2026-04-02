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
