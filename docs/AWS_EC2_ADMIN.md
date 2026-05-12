# Fidget — AWS EC2 Admin Runbook

This is the day-2 operator guide for whoever owns the EC2 box that runs
the Fidget stack in production. It assumes you already have AWS console
access and an SSH key for the instance.

For the *first-time* deploy walkthrough (security group rules, secret
rotation, smoke test), see [DEPLOY.md](./DEPLOY.md). This document
covers what to do **after** the stack is up: monitoring, restarting,
upgrading, and recovering from common failures.

---

## 1. Architecture (one-page recap)

```
                 (browsers)
                     │
                ┌────┴────┐
                │ Route53 │  (optional)
                └────┬────┘
                     │ TCP 80/443
            ┌────────▼─────────┐
            │  CloudFront CDN  │  (HLS edge cache, optional)
            └────────┬─────────┘
                     │
         ┌───────────▼───────────┐
         │    EC2 instance        │
         │  ┌──────────────────┐  │   docker compose stack
         │  │ frontend (nginx) │◄─┼── 80/443
         │  │ backend (axum)   │  │   8080  (internal)
         │  │ media-server SRS │  │   1935 RTMP, 1985 API,
         │  │                  │  │   8088 HLS, 8000/UDP RTC
         │  │ db (postgres)    │  │   5432  (internal)
         │  │ edge (nginx)     │  │   8090 HLS edge
         │  │ router (axum)    │  │   8070  (internal)
         │  └──────────────────┘  │
         └────────────────────────┘
                     │
                ┌────┴────┐
                │   S3    │  HLS origin (planned; not wired in code yet)
                └─────────┘
```

**Open ports on the EC2 security group**:

| Port    | Proto | Purpose                                      |
|---------|-------|----------------------------------------------|
| 22      | TCP   | SSH (restrict to your IP)                    |
| 80, 443 | TCP   | Public HTTP / HTTPS (frontend nginx)         |
| 1935    | TCP   | Optional: external OBS / RTMP push           |
| 8000    | UDP   | **WebRTC media — mandatory for video**       |

**Never expose**: 5432 (Postgres), 8080 (backend API), 1985 (SRS API),
8070/8090 (router/edge). The frontend nginx proxies everything users
need at the edge.

---

## 2. Routine operations

All commands assume:

```bash
ssh ec2-user@<EC2_PUBLIC_IP>
cd ~/finalproject-group10_s26
```

### 2.1 Check service health

```bash
docker compose ps
```

All six services should be `Up`. `db` should also report `(healthy)`.

```bash
docker compose logs --tail=50 backend
docker compose logs --tail=50 media-server
```

Healthy backend startup includes:
- `Database connected`
- `WebRTC candidate configured candidate=<EC2_PUBLIC_IP>`
- `Backend listening on 0.0.0.0:8080`

If backend logs show `CANDIDATE is unset or loopback`, see §4.1.

### 2.2 Restart a single service

```bash
docker compose restart backend            # or frontend / media-server / db
```

`media-server` restarts will drop any in-flight broadcasts but recover
in <10s. `db` restarts are safe — backend reconnects automatically.

### 2.3 Restart the whole stack (zero data loss)

```bash
docker compose down
docker compose up -d
```

The Postgres data and HLS segment volumes survive. Allow ~30s for the
backend to reconnect to the DB and for SRS to come up.

### 2.4 Tail live logs

```bash
docker compose logs -f backend media-server frontend
```

Useful greps:

```bash
# WHIP publish attempts and their outcome
docker compose logs media-server | grep -E "whip|on_publish"

# Backend rejection reasons (unknown stream key, role mismatch, etc.)
docker compose logs backend | grep -E "Rejected|Accepted RTMP"

# nginx 5xx responses (proxy failures)
docker compose logs frontend | grep -E " 5[0-9]{2} "
```

---

## 3. Upgrades & rollbacks

### 3.1 Pull and deploy a new release

```bash
git fetch origin
git log --oneline HEAD..origin/main      # preview what will change
git pull --ff-only origin main
docker compose build
docker compose up -d                     # recreates only changed services
```

If a migration was added (`migrations/*.sql`), it runs automatically on
backend startup — confirm in `docker compose logs backend`.

### 3.2 Roll back to the previous commit

```bash
git log --oneline -5                     # find the SHA you want
git checkout <SHA>
docker compose build
docker compose up -d
```

The DB schema is **not** automatically downgraded. If the bad release
ran a migration you cannot live with, restore from backup (§5).

---

## 4. Common failures and fixes

### 4.1 Viewers stuck on "Waiting for broadcaster"

**Most common cause**: UDP 8000 closed in the security group, or
`CANDIDATE` not set to the EC2 public IP.

```bash
# Verify SRS is advertising the right candidate
docker compose exec media-server sh -c 'echo $CANDIDATE'
# must print the EC2 public IP, not 127.0.0.1

# Verify UDP 8000 is open
nc -u -z -w2 <EC2_PUBLIC_IP> 8000 && echo "open" || echo "blocked"
```

If `CANDIDATE` is wrong:

```bash
# Edit .env, set CANDIDATE=<EC2_PUBLIC_IP>, then:
docker compose up -d --force-recreate media-server backend
```

If UDP 8000 is blocked, fix the security group:

```bash
aws ec2 authorize-security-group-ingress \
  --group-id sg-XXXXXXXX \
  --protocol udp --port 8000 --cidr 0.0.0.0/0
```

### 4.2 `WHIP signalling failed: 502`

The media-server container returned a non-2xx to the WHIP signalling
POST. Two known causes:

| Backend log line                                  | Meaning                                                          | Fix                                                  |
|---------------------------------------------------|------------------------------------------------------------------|------------------------------------------------------|
| `Rejected RTMP publish (unknown key)`             | Stream row was deleted but the streamer is using the old key     | Have the user create a new stream in the dashboard   |
| `Rejected RTMP publish because owner role …`      | The publishing user's DB role isn't `admin` or `streamer`        | Promote them via the admin UI or `UPDATE users …`    |
| (No backend log; SRS shows `RtcStreamBusy`)       | A previous WHIP session for the same key is still alive on SRS   | Wait ~30s for SRS to time it out, then retry         |

### 4.3 Streamer is "live" but viewers see nothing

```bash
docker compose exec media-server ls /tmp/hls/hls/
```

If there's a directory named after the stream key but it's empty, SRS
got the WebRTC handshake but the WebRTC→RTMP bridge isn't producing
media. Most often this is a CANDIDATE issue (§4.1) — DTLS completed
on a network the streamer can reach but media never flowed.

If there's no directory at all, on_publish never fired. Check
backend logs for `Rejected RTMP publish` (see §4.2).

### 4.4 Backend crash-looping

```bash
docker compose logs --tail=200 backend
```

Common causes:
- DB unavailable on startup → wait, or restart the db service
- Bad `JWT_SECRET` (empty) → set in `.env`, recreate backend
- Migration failure → see backend log for the offending SQL

### 4.5 Disk full

HLS segments accumulate in `/tmp/hls` inside the media-server
container. The `hls_window=300s` setting caps each active stream at
~120 MB, but inactive directories linger 30s.

```bash
docker system df
docker exec finalproject-group10_s26-media-server-1 du -sh /tmp/hls/hls/*
```

To clear stale segments without restarting:

```bash
docker compose exec media-server sh -c 'rm -rf /tmp/hls/hls/*'
```

---

## 5. Backups

### 5.1 Postgres

```bash
# Take a backup
docker compose exec -T db pg_dump -U fidget fidget > backup-$(date +%F).sql

# Copy off the box
scp ec2-user@<EC2_PUBLIC_IP>:~/finalproject-group10_s26/backup-*.sql ./
```

### 5.2 Restore

```bash
docker compose exec -T db psql -U fidget -d fidget < backup-2026-05-12.sql
```

Schedule `pg_dump` via cron or AWS Backup. The DB is small
(< 100 MB even with months of role-application history), so daily
full dumps to S3 are fine.

---

## 6. Admin user management

The bootstrap admin is created on backend startup using
`ADMIN_USERNAME` / `ADMIN_PASSWORD` from `.env`. To **change the admin
password** after deploy:

1. Log in as `admin` and use the dashboard's password-change flow, OR
2. Update `ADMIN_PASSWORD` in `.env` and restart the backend; on next
   start it will re-hash and update the row in place.

**Promote a user to streamer or admin** via SQL (no UI for it yet):

```bash
docker compose exec -T db psql -U fidget -d fidget \
  -c "UPDATE users SET role = 'streamer' WHERE username = '<name>';"
```

Roles: `viewer` (default), `streamer`, `admin`.

---

## 7. Scaling notes

The current stack is single-instance. Before scaling out:

- **Database**: move to RDS (Postgres 15+); update `DATABASE_URL` in
  `.env` and remove the `db` service from `docker-compose.yml`.
- **HLS storage**: implement the S3 publisher in `backend/src/transcode.rs`
  (currently writes to local volume) and point CloudFront at the bucket.
- **WebRTC**: SRS does not horizontally scale. For >~500 concurrent
  publishers, deploy multiple SRS instances behind a stream-key-aware
  router and update `frontend/nginx.conf` accordingly.
- **TURN**: enable the bundled coturn (`docker compose --profile turn up -d`)
  and set `TURN_URL` / `TURN_USERNAME` / `TURN_PASSWORD` in `.env` so
  viewers behind symmetric NAT can still connect.

---

## 8. Emergency contact playbook

1. **Demo is starting in <10 min and viewers see nothing**:
   - Confirm `docker compose ps` shows all services Up.
   - `docker compose logs --tail=20 media-server | grep candidate` — must show your EC2 public IP.
   - `nc -u -z <EC2_PUBLIC_IP> 8000` from your laptop must succeed.
   - If both look right, hard-refresh streamer + viewer tabs and click Go Live again.

2. **Backend is down and won't come back**:
   - `docker compose logs --tail=200 backend` — read the panic message.
   - If DB-related, `docker compose restart db backend`.
   - If config-related, edit `.env`, then `docker compose up -d --force-recreate backend`.

3. **Whole stack is wedged**:
   - `docker compose down && docker compose up -d`.
   - Wait 30s for the DB health check.
   - Verify `docker compose ps` again.

4. **Box is unreachable**:
   - Check the EC2 instance state in the AWS console.
   - Verify SSH security group still allows your IP.
   - Last resort: stop/start the instance (preserves EBS, gets new public IP — remember to update `CANDIDATE`).
