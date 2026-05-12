# Deploy Checklist (EC2 + S3 + CloudFront)

Run these on the **EC2 box** unless noted otherwise. Order matters.

## 0. Required AWS-side fixes (will not work without these)

WHIP publish and WHEP playback ride on **UDP**. Without these two
things, viewers will hang on "Waiting for broadcaster" and the streamer
will look live but produce no HLS segments.

### a. Open UDP 8000 in the EC2 security group

```bash
# Replace sg-XXXXXXXX with your instance's security group id.
aws ec2 authorize-security-group-ingress \
  --group-id sg-XXXXXXXX \
  --protocol udp --port 8000 --cidr 0.0.0.0/0
```

Also confirm these are open:

| Port      | Proto | Why                              |
|-----------|-------|----------------------------------|
| 80, 443   | TCP   | Frontend nginx (HTTP / HTTPS)    |
| 1935      | TCP   | Optional: OBS / RTMP push        |
| 8000      | UDP   | **WebRTC media (mandatory)**     |

Do **not** publish 5432 (Postgres) or 8080 (backend) to the internet.

### b. Set CANDIDATE in `.env` on the EC2 box

```bash
ssh ec2-user@<public-ip>
cd ~/finalproject-group10_s26
# Replace with the public IPv4 (or DNS) of the EC2 instance.
echo "CANDIDATE=<EC2_PUBLIC_IP>" >> .env
```

Verify it actually took effect inside the running media-server:

```bash
docker compose exec media-server grep candidate conf/srs.conf
docker compose restart media-server
```

If the backend logs show `CANDIDATE is unset or loopback` at startup,
the value didn't propagate — check `.env` and rebuild.

## 1. Rotate default secrets

The repo ships with placeholder secrets. **Before exposing the box:**

```bash
# Generate strong secrets
openssl rand -hex 32   # use for JWT_SECRET
openssl rand -hex 16   # use for POSTGRES_PASSWORD and ADMIN_PASSWORD
```

Edit `.env`:

```env
JWT_SECRET=<generated>
POSTGRES_PASSWORD=<generated>
ADMIN_PASSWORD=<generated>
DATABASE_URL=postgres://fidget:<generated>@db:5432/fidget
```

## 2. Build & start the stack

```bash
docker compose down
docker compose up -d --build
docker compose ps          # all services should be Up / healthy
docker compose logs backend | head -40
```

Expected backend log lines:
- `Database connected`
- `WebRTC candidate configured candidate=<EC2_PUBLIC_IP>`
- `Backend listening on 0.0.0.0:8080`

If you see `CANDIDATE is unset or loopback` instead, redo step 0b.

## 3. Smoke test

1. Open `http://<EC2_PUBLIC_IP>/` in Chrome.
2. Log in as `admin` / `<ADMIN_PASSWORD>`.
3. Create a stream, click **Start Capture**, then **Go Live**.
4. In a second browser (or incognito), open the watch page for that stream.
5. WHEP should bind within ~1.5s; if UDP 8000 is blocked the viewer
   silently falls back to HLS within ~2-3s.

## 4. CloudFront / S3 (optional for demo)

The router already advertises CloudFront when `CLOUDFRONT_DOMAIN` is set:

```env
CLOUDFRONT_DOMAIN=https://<your-distribution>.cloudfront.net
```

The HLS publisher sidecar (S3 upload) is **not implemented in code** — for
the demo, point CloudFront at the EC2 edge nginx as origin
(`http://<EC2_PUBLIC_IP>:8090`) and viewers will get cached HLS without
any extra infra.

## Common failure modes

| Symptom                                            | Cause                                                   | Fix                              |
|----------------------------------------------------|---------------------------------------------------------|----------------------------------|
| Viewer stuck on "Waiting for broadcaster"          | UDP 8000 closed → WHIP publish never reaches SRS        | Step 0a                          |
| Streamer "stream ended" when viewer refreshes      | (Pre-fix) `on_publish_done` was synchronous             | Already debounced 3s             |
| WHEP times out, HLS plays after a beat             | UDP 8000 closed *for the viewer* but publish worked     | Step 0a (still required)         |
| `502` from `/whip/`                                | media-server container down or `rtc_to_rtmp off`        | `docker compose restart media-server` |
| Streamer looks live but no HLS segments on disk    | SRS never bridged WebRTC→RTMP (UDP blocked)             | Step 0a                          |
