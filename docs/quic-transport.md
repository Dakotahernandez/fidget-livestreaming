# QUIC Unreliable Datagrams for Live Video Ingest

## Why QUIC instead of TCP?

TCP guarantees in-order, reliable delivery — great for web pages and API calls, terrible for real-time video. When a TCP packet is lost, **every subsequent packet is blocked** until the lost one is retransmitted. This is called **head-of-line (HOL) blocking**, and it causes visible stuttering in live video because fresh frames pile up behind a stale retransmission that viewers don't even care about anymore.

### The problem with TCP for live video

```mermaid
sequenceDiagram
    participant Browser
    participant Server
    Browser->>Server: Frame 1 (ok)
    Browser->>Server: Frame 2 (ok)
    Browser-xServer: Frame 3 (lost!)
    Browser->>Server: Frame 4 (queued — blocked)
    Browser->>Server: Frame 5 (queued — blocked)
    Note over Server: Waiting for Frame 3 retransmit...<br/>Frames 4 & 5 are stale by the time they arrive
    Browser->>Server: Frame 3 (retransmitted)
    Server->>Server: Now delivers 3, 4, 5 — all late
```

For live streaming, a dropped video frame is **less harmful** than delaying all future frames. The viewer would rather see a brief glitch than a frozen stream.

## QUIC Unreliable Datagrams (RFC 9221)

QUIC is a modern transport protocol built on UDP. While QUIC normally provides reliable, ordered streams (like TCP but without HOL blocking between streams), it also supports an extension called **unreliable datagrams** defined in [RFC 9221](https://datatracker.ietf.org/doc/html/rfc9221).

Unreliable datagrams give us the best of both worlds:

| Property | TCP | QUIC Streams | QUIC Datagrams |
|----------|-----|-------------|----------------|
| Encryption | TLS 1.2/1.3 | TLS 1.3 | TLS 1.3 |
| Ordered delivery | Yes | Yes (per-stream) | No |
| Reliable delivery | Yes | Yes | **No** |
| Head-of-line blocking | Yes | No (between streams) | **No** |
| Congestion control | TCP CC | QUIC CC | QUIC CC |
| Connection setup | 1-2 RTT | **0-1 RTT** | **0-1 RTT** |

Key advantages over raw UDP:
- **Still encrypted** (TLS 1.3) — no plaintext video on the wire
- **Still congestion-controlled** — won't flood the network
- **0-RTT connection setup** — faster than TCP's 3-way handshake + TLS
- **Connection migration** -- survives IP changes (e.g., switching WiFi to cellular)

## How We Use It: WebTransport

[WebTransport](https://www.w3.org/TR/webtransport/) is a browser API that exposes QUIC features — including unreliable datagrams — to JavaScript. It runs over HTTP/3 and is supported in Chrome 97+, Edge 97+, and Firefox 114+.

### Data flow

```mermaid
graph LR
    A[Browser<br/>MediaRecorder] -->|WebTransport<br/>QUIC Datagrams| B[Rust Backend<br/>wtransport]
    B -->|stdin pipe<br/>reassembled chunks| C[ffmpeg]
    C -->|RTMP/FLV| D[nginx-rtmp<br/>media server]
    D -->|HLS segments| E[Edge Server]
    E -->|HTTP| F[Viewers]

    style A fill:#1a1a2e,stroke:#c6f135,color:#fff
    style B fill:#1a1a2e,stroke:#c6f135,color:#fff
    style C fill:#1a1a2e,stroke:#e63946,color:#fff
    style D fill:#1a1a2e,stroke:#457b9d,color:#fff
    style E fill:#1a1a2e,stroke:#457b9d,color:#fff
    style F fill:#1a1a2e,stroke:#a8dadc,color:#fff
```

### Connection establishment

The browser needs to trust the server's self-signed certificate. We solve this by having the backend expose the certificate's SHA-256 hash via a REST endpoint, which the browser fetches before opening the WebTransport session.

```mermaid
sequenceDiagram
    participant Browser
    participant Backend as Backend (Axum REST)
    participant QUIC as Backend (QUIC Server)

    Browser->>Backend: GET /api/transport/cert-hash
    Backend-->>Browser: { algorithm: "sha-256", hash: "a1b2c3..." }
    Browser->>QUIC: WebTransport handshake<br/>(serverCertificateHashes: [sha-256: a1b2c3...])
    QUIC-->>Browser: Connection established (0-1 RTT)
    loop Every 250ms
        Browser->>QUIC: Datagram(s) with video chunk fragments
    end
```

## Fragment Protocol

QUIC datagrams have a maximum size (typically ~1,200 bytes). A single MediaRecorder chunk can be 10–100 KB. We split each chunk into **fragments** with an 8-byte header:

```
┌─────────────────────────────────────────────────────────┐
│                   QUIC Datagram                         │
├──────────┬──────────┬──────────┬────────────────────────┤
│ chunk_id │frag_index│frag_total│       payload          │
│  4 bytes │ 2 bytes  │ 2 bytes  │   ≤1,192 bytes        │
│ (u32 BE) │(u16 BE)  │(u16 BE)  │                        │
└──────────┴──────────┴──────────┴────────────────────────┘
```

```mermaid
graph TD
    A[MediaRecorder chunk<br/>~50 KB] --> B{Split into fragments}
    B --> C[Datagram 0<br/>header + 1192 bytes]
    B --> D[Datagram 1<br/>header + 1192 bytes]
    B --> E[...]
    B --> F[Datagram N<br/>header + remaining bytes]

    G[Server receives datagrams] --> H{All fragments<br/>for chunk_id?}
    H -->|Yes| I[Reassemble in order<br/>then pipe to ffmpeg]
    H -->|No| J[Wait for more<br/>or evict if stale]

    style A fill:#1a1a2e,stroke:#c6f135,color:#fff
    style I fill:#1a1a2e,stroke:#c6f135,color:#fff
```

### Fragment reassembly on the server

The server maintains a `HashMap<chunk_id, fragments>`. When all fragments for a chunk arrive, they are reassembled in order and piped to ffmpeg. Stale incomplete chunks (when newer chunk_ids arrive) are evicted to prevent memory buildup. At most 3 pending chunks are kept in memory.

### Why this is better than TCP for our use case

```mermaid
graph TB
    subgraph TCP["TCP (WebSocket)"]
        direction TB
        T1[Frame 1] --> T2[Frame 2]
        T2 --> T3["Frame 3 (X) lost"]
        T3 -->|retransmit + block| T4[Frame 4 delayed]
        T4 --> T5[Frame 5 delayed]
    end

    subgraph QUIC["QUIC (Datagrams)"]
        direction TB
        Q1[Frame 1] --> Q2[Frame 2]
        Q2 --> Q3["Frame 3 (X) lost"]
        Q3 --> Q4["Frame 4 (ok) arrives immediately"]
        Q4 --> Q5["Frame 5 (ok) arrives immediately"]
    end

    style TCP fill:#2d0000,stroke:#e63946,color:#fff
    style QUIC fill:#002d00,stroke:#c6f135,color:#fff
```

With QUIC datagrams:
- Lost frames are simply **skipped** — the server uses the next available chunk
- Fresh frames are **never delayed** by stale retransmissions
- The viewer sees a brief glitch instead of a frozen stream

## Implementation Details

### Backend (Rust)

- **Crate:** [`wtransport`](https://crates.io/crates/wtransport) v0.7 with `self-signed` feature
- **Identity:** `Identity::self_signed(["localhost", "127.0.0.1", "::1"])` generates a TLS certificate trusted by the browser via hash pinning
- **Port:** UDP 4443 (configurable via `QUIC_PORT` env var)
- **Auth:** JWT token passed in the WebTransport URL query string, validated before accepting the session

### Frontend (JavaScript)

- **API:** `WebTransport` browser API with `serverCertificateHashes` for self-signed cert trust
- **Fragmentation:** Chunks from `MediaRecorder.ondataavailable` are split into ~1,200-byte datagrams with the fragment header
- **Writer:** `transport.datagrams.writable.getWriter()` for sending datagrams

### Docker

- Backend container exposes `4443/udp` alongside the existing `8080/tcp` for the REST API

## Browser Compatibility

| Browser | WebTransport | Unreliable Datagrams |
|---------|-------------|---------------------|
| Chrome 97+ | Yes | Yes |
| Edge 97+ | Yes | Yes |
| Firefox 114+ | Yes | Yes |
| Safari | No | No |

> **Note:** Safari does not support WebTransport yet. For Safari users, a WebSocket fallback could be added in the future.
