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

| Sprint | Dates              | Theme                                        |
|--------|--------------------|----------------------------------------------|
| 1      | Mar 31 – Apr 6     | Backend Core & Auth                          |
| 2      | Apr 7 – Apr 13     | Stream Management, Media Pipeline & Cloud Setup |
| 3      | Apr 14 – Apr 20    | Frontend, Viewer Experience & Cloud Deploy   |
| 4      | Apr 21 – Apr 27    | CDN Routing, Edge Delivery & Multi-Location  |
| 5      | Apr 28 – May 4     | Testing, Monitoring & Polish                 |
| 6      | May 5 – May 9      | Presentation, Docs & Final Fixes             |

---

## Sprint 1 — Backend Core & Auth (Mar 31 – Apr 6)

**Goal:** A running backend that connects to Postgres, handles user registration/login with JWT, and has proper error handling.

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
  - `POST /api/auth/register` — hash password with bcrypt, insert user, return JWT
  - `POST /api/auth/login` — verify password, return JWT
- [ ] **JWT middleware** (`backend/src/middleware/auth.rs`)
  - Extract and validate JWT from `Authorization: Bearer <token>`
  - Inject authenticated `UserId` into request extensions
- [ ] **Wire it all up in `main.rs`**
  - Initialize config, DB pool, tracing
  - Mount auth routes
  - Add CORS middleware via `tower-http`
- [ ] **Verify with manual testing**
  - Register a user via curl/Postman
  - Login and get a token
  - Access a protected endpoint

### Definition of Done
- `docker-compose up --build` succeeds
- Can register, login, and receive a valid JWT
- Protected routes reject unauthenticated requests

---

## Sprint 2 — Stream Management, Media Pipeline & Cloud Setup (Apr 7 – Apr 13)

**Goal:** Streamers can create streams, get stream keys, and push RTMP video that gets validated and converted to HLS. Cloud infrastructure is provisioned and ready.

### Tasks

- [ ] **Stream model** (`backend/src/models/stream.rs`)
  - Structs: `Stream`, `CreateStream`
  - DB queries: `create_stream()`, `list_live_streams()`, `find_by_stream_key()`, `set_live()`, `set_offline()`
- [ ] **Stream routes** (`backend/src/routes/streams.rs`)
  - `POST /api/streams` — create a new stream (auth required), generate unique stream key
  - `GET /api/streams` — list all live streams
  - `GET /api/streams/:id` — get stream details
  - `PUT /api/streams/:id/start` — mark stream as live
  - `PUT /api/streams/:id/stop` — mark stream as offline
- [ ] **Stream key validation** (`media-server/scripts/on_publish.sh`)
  - On RTMP publish, call backend API to validate the stream key
  - Accept/reject the connection based on response
  - Mark stream as live on successful publish
- [ ] **Media server `on_publish` integration** (`media-server/nginx.conf`)
  - Enable `on_publish` callback to the validation script
  - Enable `on_publish_done` to mark stream offline
- [ ] **Verify end-to-end RTMP → HLS**
  - Push a test stream with FFmpeg using a valid stream key
  - Confirm HLS segments appear at `http://localhost:8088/hls/<key>/index.m3u8`
  - Confirm invalid keys are rejected
- [ ] **Cloud provider setup** — *Start early — this is the hardest part*
  - Pick provider (see Cloud Provider Recommendations below)
  - Create account, claim student credits
  - Provision a VM / instance (Ubuntu, 2+ vCPU, 4+ GB RAM)
  - Install Docker & Docker Compose on the VM
  - Open firewall ports: 22 (SSH), 80, 443, 1935 (RTMP), 3000, 8080, 8088
  - Verify `docker-compose up` runs on the remote VM with the current codebase

### Definition of Done
- Stream CRUD endpoints work behind auth
- RTMP push with valid stream key produces HLS segments
- Invalid stream keys are rejected at the RTMP level
- Cloud VM is provisioned with Docker running and ports open

---

## Sprint 3 — Frontend, Viewer Experience & Cloud Deploy (Apr 14 – Apr 20)

**Goal:** Users can register, log in, browse live streams, and watch them via HLS in the browser. The platform is live on the cloud.

### Tasks

- [ ] **API service layer** (`frontend/src/services/api.js`)
  - Auth methods: `register()`, `login()`, `getProfile()`
  - Stream methods: `listStreams()`, `getStream()`, `createStream()`
  - JWT token storage/attachment (localStorage + Axios interceptor)
- [ ] **Auth state management** (`frontend/src/hooks/`)
  - `useAuth` hook — login, logout, token state, current user
- [ ] **Registration page** (`frontend/src/pages/Register.jsx`)
  - Form with username, email, password fields
  - Call register API, redirect to login on success
- [ ] **Login page** (`frontend/src/pages/Login.jsx`)
  - Form with username, password fields
  - Store JWT, redirect to dashboard on success
- [ ] **Home page** (`frontend/src/pages/Home.jsx`)
  - Fetch and display list of live streams
  - Each stream links to its watch page
- [ ] **Dashboard page** (`frontend/src/pages/Dashboard.jsx`)
  - Show user's stream key (create stream if none exists)
  - Display streaming instructions (OBS/FFmpeg)
  - Start/stop stream controls
- [ ] **Watch page** (`frontend/src/pages/Watch.jsx`)
  - HLS video player (use `hls.js` library)
  - Display stream title, streamer name
  - Auto-connect to the HLS endpoint
- [ ] **App routing** (`frontend/src/App.jsx`)
  - Wire up React Router routes for all pages
  - Add navigation bar with auth-aware links
- [ ] **Basic styling** (`frontend/src/styles/`)
  - Clean, functional CSS for all pages
- [ ] **Deploy to cloud** — *Don't delay — deploy what you have*
  - Push Docker images to cloud VM (or use `git clone` + build on VM)
  - Update `.env` with production values (public IP, DB creds)
  - Run `docker-compose up -d` on cloud VM
  - Verify all services accessible from public IP
  - Set up a basic deploy script or CI pipeline

### Definition of Done
- Full user flow works: register → login → dashboard → start stream → watch
- Viewers can watch live HLS streams in the browser
- Navigation between pages works cleanly
- Platform accessible from a public cloud URL

---

## Sprint 4 — CDN Routing, Edge Delivery & Multi-Location Testing (Apr 21 – Apr 27)

**Goal:** Edge server proxies/caches HLS content, router directs viewers to the best edge, and multi-location testing validates the CDN architecture.

### Tasks

- [ ] **Edge server HLS delivery** (`edge/nginx.conf`)
  - Proxy-pass HLS requests to the media server
  - Enable `proxy_cache` for HLS segments (.ts) and playlists (.m3u8)
  - Add proper CORS headers for cross-origin HLS playback
- [ ] **Router logic** (`router/src/main.rs`)
  - `GET /api/route/:stream_key` — return the best edge URL for a viewer
  - Health check polling of edge servers
  - Basic selection strategy (round-robin or latency-based)
  - Track active viewer counts per edge
- [ ] **Frontend integration with router**
  - Watch page queries router for the best edge URL before connecting
  - Fallback to media-server direct if router/edge unavailable
- [ ] **Deploy CDN layer to cloud**
  - Deploy edge server(s) on cloud (ideally in a second region/VM)
  - Update router config with cloud edge URLs
  - Verify edge caching works in production
- [ ] **Multi-location testing**
  - Test with VPN from different geographic locations
  - Verify edge caching reduces origin load
  - Document latency/performance observations
  - Collect data for presentation (screenshots, latency numbers)
- [ ] **Milestone check-in prep**
  - Prepare brief status summary for Dr. Freeman meeting
  - Have a working demo ready to show

### Definition of Done
- Edge server caches and serves HLS segments
- Router health-checks edges and directs viewers
- CDN layer deployed to cloud
- VPN-based multi-location test completed with documented results
- Milestone check-in with Dr. Freeman completed

---

## Sprint 5 — Testing, Monitoring & Polish (Apr 28 – May 4)

**Goal:** The platform is stable, monitored, tested, and documented.

### Tasks

- [ ] **Backend integration tests**
  - Auth flow tests (register, login, invalid credentials)
  - Stream CRUD tests
  - Stream key validation tests
- [ ] **Viewer simulation** (`scripts/simulate_viewers.sh`)
  - Script to simulate N concurrent viewers pulling HLS segments
  - Collect metrics on latency, errors, throughput
- [ ] **Prometheus monitoring** (`monitoring/prometheus.yml`)
  - Add scrape targets for backend, router, edge
  - Expose `/metrics` endpoint on backend and router
- [ ] **Grafana dashboard** (`monitoring/grafana/dashboards/streaming.json`)
  - Panels: active streams, viewer count, edge cache hit ratio, latency
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

### Definition of Done
- All tests pass
- No clippy warnings
- Monitoring dashboards show real data
- Documentation is complete and accurate

---

## Sprint 6 — Presentation, Docs & Final Fixes (May 5 – May 9)

**Goal:** Ship it. Present it. Be proud of it.

### Tasks

- [ ] **Final bug fixes & polish**
  - Fix any remaining issues from Sprint 5
  - Ensure `docker-compose up --build` works cleanly from scratch
- [ ] **README answers**
  - Answer all required submission questions in the README
  - Document AI tool usage
- [ ] **Presentation slides**
  - Problem introduction: why live streaming + CDN?
  - Architecture overview with diagrams
  - Technical deep dives (RTMP→HLS pipeline, JWT auth, CDN routing)
  - Demo (live or pre-recorded screen capture)
  - Challenges faced
  - What we learned
  - Next steps
- [ ] **Demo recording** (backup)
  - Record a screen capture of the full user flow as backup
  - Include: register, login, start stream, watch from multiple "locations"
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
| Cloud deployment problems               | Medium     | High   | Start cloud research in Sprint 3; use Docker images   |
| Team member availability conflicts      | Low        | Medium | Plan buffer in Sprint 5; track blockers in standups   |
| Scope creep on frontend polish          | Medium     | Low    | Keep UI functional, not fancy; prioritize core flow   |
| Edge caching not working as expected    | Medium     | Medium | Test with simple static files first; fallback to direct|

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
