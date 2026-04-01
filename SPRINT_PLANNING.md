# Fidget — Sprint Planning

> **Deadline:** May 9, 2026  
> **Start Date:** March 31, 2026  
> **Duration:** ~5.5 weeks (6 sprints, ~6 days each)  
> **Project:** One-to-many live streaming platform with multi-server CDN delivery

---

## Current State (March 31)

| Component              | Status          |
|------------------------|-----------------|
| Docker / Infra         | Done             |
| Database Schema        | Done             |
| Backend Server Boot    | Skeleton only    |
| Backend Business Logic | All stubs        |
| Frontend Pages         | All stubs        |
| Media Server (RTMP/HLS)| Config works, no auth |
| Edge Server            | Placeholder      |
| Router (CDN logic)     | Stub             |
| Cloud Deployment       | Not started      |
| Monitoring             | Placeholder      |
| Testing                | Not started      |
| Documentation          | Placeholders     |
| Presentation           | Not started      |

---

## Sprint Overview

| Sprint | Dates              | Theme                                           |
|--------|--------------------|------------------------------------------------|
| 1      | Mar 31 -- Apr 6    | Backend + Frontend Basics (get it out of the way) |
| 2      | Apr 7 -- Apr 13    | RTMP-to-HLS Pipeline & Stream Key Auth          |
| 3      | Apr 14 -- Apr 20   | Edge Caching & CDN Router Logic                  |
| 4      | Apr 21 -- Apr 27   | Cloud Deploy, Multi-Location Testing & Milestone |
| 5      | Apr 28 -- May 4    | Monitoring, Load Testing & Networking Polish     |
| 6      | May 5 -- May 9     | Presentation, Docs & Final Fixes                 |

---

## Sprint 1 -- Backend + Frontend Basics (Mar 31 -- Apr 6)

**Goal:** Get ALL the boilerplate out of the way fast. Working backend with auth, stream CRUD, and a functional frontend. None of this is the graded networking work -- just the scaffolding to support it.

### Tasks

- [ ] **Config loading** (`backend/src/config.rs`)
  - Load `DATABASE_URL`, `JWT_SECRET`, `RUST_LOG` from environment
  - Use a struct with a `from_env()` constructor
- [ ] **Database pool** (`backend/src/db.rs`)
  - Set up `sqlx::PgPool` connection pool
  - Expose an `init_pool()` function
- [ ] **Error types** (`backend/src/errors.rs`)
  - Define `AppError` enum (NotFound, Unauthorized, BadRequest, Internal)
  - Implement `IntoResponse` for Axum integration
- [ ] **User model** (`backend/src/models/user.rs`)
  - Structs: `User`, `CreateUser`, `LoginUser`
  - DB queries: `create_user()`, `find_by_username()`, `find_by_id()`
- [ ] **Auth routes** (`backend/src/routes/auth.rs`)
  - `POST /api/auth/register` -- hash password with bcrypt, insert user, return JWT
  - `POST /api/auth/login` -- verify password, return JWT
- [ ] **JWT middleware** (`backend/src/middleware/auth.rs`)
  - Extract and validate JWT from `Authorization: Bearer <token>`
  - Inject authenticated `UserId` into request extensions
- [ ] **Stream model** (`backend/src/models/stream.rs`)
  - Structs: `Stream`, `CreateStream`
  - DB queries: `create_stream()`, `list_live_streams()`, `find_by_stream_key()`, `set_live()`, `set_offline()`
- [ ] **Stream routes** (`backend/src/routes/streams.rs`)
  - `POST /api/streams` -- create a new stream (auth required), generate unique stream key
  - `GET /api/streams` -- list all live streams
  - `GET /api/streams/:id` -- get stream details
  - `PUT /api/streams/:id/start` -- mark stream as live
  - `PUT /api/streams/:id/stop` -- mark stream as offline
- [ ] **Wire it all up in `main.rs`**
  - Initialize config, DB pool, tracing
  - Mount auth + stream routes
  - Add CORS middleware via `tower-http`
- [ ] **Frontend -- all pages** (keep it simple, functional only)
  - API service layer (`frontend/src/services/api.js`) -- auth + stream methods, JWT interceptor
  - `useAuth` hook (`frontend/src/hooks/`)
  - Registration page -- form, calls API
  - Login page -- form, stores JWT
  - Home page -- lists live streams
  - Dashboard -- shows stream key, OBS instructions
  - Watch page -- HLS player with `hls.js`
  - App routing (`frontend/src/App.jsx`) -- wire up routes + nav bar
  - Minimal styling -- clean and functional, don't overthink it
- [ ] **Verify end-to-end locally**
  - `docker-compose up --build` succeeds
  - Register, login, create stream all work
  - Frontend pages render and connect to API

### Definition of Done
- `docker-compose up --build` runs clean
- Full user flow works: register, login, dashboard, create stream
- Backend auth + stream CRUD verified with curl
- Frontend is functional (not pretty -- just working)

---

## Sprint 2 -- RTMP-to-HLS Pipeline & Stream Key Auth (Apr 7 -- Apr 13)

**Goal:** The core networking feature: push RTMP video, validate the stream key against the backend, transcode to HLS, and serve it to viewers. This is where the real project starts.

### Tasks

- [ ] **Stream key validation** (`media-server/scripts/on_publish.sh`)
  - On RTMP publish, call backend API to validate the stream key
  - Accept/reject the RTMP connection based on response
  - Mark stream as live on successful publish
  - Mark stream as offline on `on_publish_done`
- [ ] **Media server `on_publish` integration** (`media-server/nginx.conf`)
  - Enable `on_publish` callback pointing to the validation script
  - Enable `on_publish_done` callback for cleanup
  - Tune HLS settings: segment length, playlist depth, fragment naming
- [ ] **Verify end-to-end RTMP -> HLS locally**
  - Push a test stream with FFmpeg using a valid stream key
  - Confirm HLS segments appear at `http://localhost:8088/hls/<key>/index.m3u8`
  - Confirm invalid keys are rejected at the RTMP level
  - Watch the stream in the frontend Watch page
- [ ] **HLS playback tuning**
  - Test latency from RTMP push to HLS playback
  - Adjust segment duration and playlist size for acceptable latency
  - Document observed latency numbers
- [ ] **Cloud provider setup** -- *Start early, this is the hardest part*
  - Pick provider (Oracle Cloud Free Tier or DigitalOcean)
  - Create account, claim student credits
  - Provision a VM / instance (Ubuntu, 2+ vCPU, 4+ GB RAM)
  - Install Docker & Docker Compose on the VM
  - Open firewall ports: 22 (SSH), 80, 443, 1935 (RTMP), 3000, 8080, 8088
  - Verify `docker-compose up` runs on the remote VM

### Definition of Done
- RTMP push with valid stream key produces HLS segments
- Invalid stream keys are rejected at the RTMP level
- HLS stream watchable in browser with measured latency
- Cloud VM is provisioned with Docker running and ports open

---

## Sprint 3 -- Edge Caching & CDN Router Logic (Apr 14 -- Apr 20)

**Goal:** Build the CDN layer: edge servers that cache and serve HLS content, and a router that directs viewers to the best edge. This is the second core networking feature.

### Tasks

- [ ] **Edge server HLS delivery** (`edge/nginx.conf`)
  - Proxy-pass HLS requests to the origin media server
  - Enable `proxy_cache` for HLS segments (.ts files)
  - Configure cache for playlists (.m3u8) with short TTL (segments change frequently)
  - Add proper CORS headers for cross-origin HLS playback
  - Expose cache hit/miss status via response headers (for debugging/metrics)
- [ ] **Test edge caching locally**
  - Push a stream, pull HLS from edge instead of origin
  - Verify cache HIT vs MISS in response headers
  - Measure latency improvement from cached segments
- [ ] **Router logic** (`router/src/main.rs`)
  - `GET /api/route/:stream_key` -- return the best edge URL for a viewer
  - Health check polling of edge servers (periodic HTTP checks)
  - Basic selection strategy (round-robin or least-connections)
  - Track active viewer counts per edge
  - Return degraded response if no healthy edges (fallback to origin)
- [ ] **Frontend integration with router**
  - Watch page queries router for the best edge URL before connecting
  - Fallback to media-server direct if router/edge unavailable
- [ ] **Deploy full stack to cloud**
  - Push code to cloud VM, run `docker-compose up -d`
  - Update `.env` with production values (public IP, DB creds)
  - Verify all services accessible from public IP
  - Test RTMP push to cloud + HLS playback from cloud

### Definition of Done
- Edge server caches HLS segments, measurable cache hits
- Router health-checks edges and directs viewers
- Frontend uses router to pick edge server
- Full stack running on cloud with public access

---

## Sprint 4 -- Cloud CDN Deploy, Multi-Location Testing & Milestone (Apr 21 -- Apr 27)

**Goal:** Deploy edge servers to multiple cloud locations, test with VPN from different regions, collect performance data. Prepare for Dr. Freeman milestone check-in.

### Tasks

- [ ] **Multi-edge cloud deployment**
  - Deploy edge server(s) in a second cloud region or separate VM
  - Register both edges with the router
  - Verify router distributes viewers across edges
- [ ] **Multi-location VPN testing**
  - Test from 3+ VPN locations (e.g., US East, US West, Europe)
  - Measure and record: time-to-first-byte, segment download latency, buffering events
  - Compare direct-from-origin vs. through-edge performance
  - Screenshot/record results for presentation
- [ ] **Edge cache performance analysis**
  - Verify edge caching reduces load on origin server
  - Measure cache hit ratios under concurrent viewers
  - Test cache behavior during stream start/stop transitions
- [ ] **Network resilience testing**
  - Simulate edge server failure, verify router removes it from rotation
  - Test viewer failover to alternate edge or origin
  - Document failover behavior and timing
- [ ] **Milestone check-in prep**
  - Prepare brief status summary for Dr. Freeman meeting
  - Have a working cloud demo ready to show
  - Prepare talking points on architecture decisions

### Definition of Done
- Edge servers running in 2+ cloud locations
- VPN multi-location test completed with documented latency data
- Router failover tested and documented
- Milestone check-in with Dr. Freeman completed

---

## Sprint 5 -- Monitoring, Load Testing & Networking Polish (Apr 28 -- May 4)

**Goal:** Add observability to the networking layer, stress-test under load, and polish the CDN behavior. This is where you prove the system works.

### Tasks

- [ ] **Prometheus monitoring** (`monitoring/prometheus.yml`)
  - Add scrape targets for backend, router, edge
  - Expose `/metrics` endpoint on backend and router (request count, latency histograms)
  - Track edge cache hit ratio, active connections, bytes served
- [ ] **Grafana dashboard** (`monitoring/grafana/dashboards/streaming.json`)
  - Panels: active streams, viewer count per edge, cache hit ratio, request latency
  - Panel: origin vs. edge traffic comparison
- [ ] **Viewer simulation** (`scripts/simulate_viewers.sh`)
  - Script to simulate N concurrent viewers pulling HLS segments
  - Collect metrics on latency, errors, throughput
  - Test with 10, 50, 100 simulated viewers
  - Document how the system scales
- [ ] **Networking polish**
  - Tune RTMP/HLS segment duration for optimal latency vs. reliability
  - Tune edge cache TTLs based on observed behavior
  - Optimize router health-check intervals
- [ ] **Error handling audit**
  - Remove any remaining `unwrap()` calls in production code
  - Ensure all errors propagate cleanly with `?`
  - Verify no `unsafe` code exists
- [ ] **Code quality pass**
  - Run `cargo clippy` and fix all warnings
  - Run `cargo fmt` for consistent formatting
  - Add doc comments (`///`) to all public functions
- [ ] **Documentation**
  - Fill in `docs/architecture.md` with system design, data flow diagrams
  - Fill in `docs/testing.md` with test plan and results
  - Document networking decisions: why HLS, why this cache strategy, why this routing algorithm

### Definition of Done
- Monitoring dashboards show real data from cloud deployment
- Load testing results documented (latency at N viewers)
- No clippy warnings, no `unwrap()` in production code
- Architecture and testing docs complete

---

## Sprint 6 -- Presentation, Docs & Final Fixes (May 5 -- May 9)

**Goal:** Ship it. Present it. Be proud of it.

### Tasks

- [ ] **Final bug fixes & polish**
  - Fix any remaining issues from Sprint 5
  - Ensure `docker-compose up --build` works cleanly from scratch
- [ ] **README answers**
  - Answer all required submission questions in the README
  - Document AI tool usage
- [ ] **Presentation slides** (focus on networking)
  - Problem introduction: why live streaming + CDN?
  - Architecture overview with diagrams
  - Technical deep dives: RTMP->HLS pipeline, stream key validation, CDN routing, edge caching
  - Performance data: latency numbers, cache hit ratios, multi-location results
  - Demo (live or pre-recorded screen capture)
  - Challenges faced
  - What we learned
  - Next steps
- [ ] **Demo recording** (backup)
  - Record a screen capture of the full user flow as backup
  - Include: push RTMP stream, watch from edge, VPN multi-location, monitoring dashboards
- [ ] **Rehearse presentation**
  - Practice ~12 minutes, ensure each team member speaks
  - Time the demo portion
- [ ] **Individual submissions**
  - Each member answers individual contribution questions on Teams

### Definition of Done
- Code pushed to main branch
- README fully filled out
- Slides finalized
- Demo works (live or recorded)
- All individual submissions done

---

## Risk Register

| Risk                                    | Likelihood | Impact | Mitigation                                          |
|-----------------------------------------|------------|--------|------------------------------------------------------|
| RTMP/HLS pipeline issues                | Medium     | High   | Test early in Sprint 2; have FFmpeg fallback ready    |
| Cloud deployment problems               | Medium     | High   | Start cloud setup in Sprint 2; use Docker images      |
| Edge caching not working as expected    | Medium     | High   | Test with simple static files first; fallback to direct|
| Router failover edge cases              | Medium     | Medium | Test failure scenarios explicitly in Sprint 4         |
| Team member availability conflicts      | Low        | Medium | Plan buffer in Sprint 5; track blockers in standups   |
| Scope creep on frontend polish          | Medium     | Low    | Frontend done in Sprint 1; don't touch it again       |

---

## Cloud Provider Recommendations

| Provider | Free Credits | Best For | Notes |
|----------|-------------|----------|-------|
| **AWS (Academy/Learner Lab)** | Varies by school | Best if your course provides it | Check with Dr. Freeman — many CS courses include AWS Academy access |
| **Oracle Cloud Free Tier** | Always free: 4 OCPU, 24 GB RAM (ARM) | Most free compute, $0 forever | Generous enough to run the entire stack; less community docs |
| **DigitalOcean** | $200 via GitHub Student Pack | Easiest Docker deployment | Simple UI, great docs, `doctl` CLI |
| **GCP** | $300 for 90 days (new accounts) | Good all-around | Requires credit card; auto-charges after credits expire |
| **Azure for Students** | $100 student credits | No credit card needed | Portal can be confusing; decent VM options |
| **Railway / Fly.io** | Small free tiers | Quick demos, easy Docker deploy | May not have enough resources for RTMP + HLS |

**Recommended path:**
1. **Oracle Cloud free tier** gives the most compute at $0 — permanently free ARM instances
2. **DigitalOcean** via GitHub Student Developer Pack ($200 credits, easiest experience)
3. Team to evaluate both and decide by Sprint 2

---

## Notes

- **Milestone check-in with Dr. Freeman** must happen before end of April (aim for Sprint 4, by Apr 27)
- **Cloud deployment starts in Sprint 2** — provision early, deploy in Sprint 3, iterate in Sprint 4
- **AI tool usage must be documented** for the presentation
- Frontend doesn't need to be beautiful — functional and clear is fine
- Focus on the networking aspects (RTMP, HLS, CDN, routing) — that's where the grade is
