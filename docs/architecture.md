# Architecture

## End-to-End Streaming Pipeline

This diagram shows the full path of a live stream from the **streamer** through
**ingest**, **transcoding**, the **control plane** (auth, geo-routing,
metrics), out to **edge PoPs**, and finally to **viewers** on different client
form factors.

```mermaid
flowchart LR
    %% ===== Streamer (origin) =====
    subgraph SRC["Streamer (Origin)"]
        direction TB
        ST[["Streamer Browser<br/>React + MediaRecorder<br/>WebTransport (QUIC)"]]
    end

    %% ===== Ingest tier =====
    subgraph INGEST["Ingest Tier"]
        direction TB
        QUIC["QUIC Ingest Server<br/>Rust + wtransport<br/>:4443/udp"]
        FF["ffmpeg Transcoder<br/>WebM → H.264 / AAC"]
        RTMP["nginx-rtmp<br/>RTMP → HLS<br/>:1935"]
        HLS[("HLS Segments<br/>shared volume")]
    end

    %% ===== Control plane =====
    subgraph CTRL["Control Plane"]
        direction TB
        API["Axum REST API<br/>auth · streams · cert-hash<br/>:8080"]
        DB[("PostgreSQL 16<br/>users · streams")]
        ROUTER["Geo Router<br/>MaxMind GeoLite2<br/>:8070"]
        MON["Prometheus + Grafana<br/>metrics & dashboards"]
    end

    %% ===== Edge / CDN tier =====
    subgraph EDGES["Edge Delivery Tier (PoPs)"]
        direction TB
        E1["Edge PoP — US-East<br/>nginx :8090"]
        E2["Edge PoP — US-West<br/>nginx :8090"]
        E3["Edge PoP — EU<br/>nginx :8090"]
    end

    %% ===== Clients =====
    subgraph CLIENTS["Viewers / Clients"]
        direction TB
        CW["Web Browser<br/>HLS.js Player"]
        CM["Mobile Browser<br/>native HLS"]
        CT["Smart TV / OTT App<br/>HLS"]
    end

    %% ----- Streamer control flow -----
    ST -- "REST: login, create stream,<br/>fetch cert-hash (JWT)" --> API
    API -- "SQL" --> DB

    %% ----- Streamer media flow -----
    ST == "QUIC datagrams<br/>(fragmented WebM)" ==> QUIC
    QUIC -- "validate JWT &<br/>stream ownership" --> DB
    QUIC == "reassembled chunks<br/>(stdin pipe)" ==> FF
    FF == "RTMP / FLV" ==> RTMP
    RTMP == "writes .m3u8 + .ts" ==> HLS

    %% ----- Fan-out to edges -----
    HLS == "pull / shared volume" ==> E1
    HLS == "pull / shared volume" ==> E2
    HLS == "pull / shared volume" ==> E3

    %% ----- Viewer routing -----
    CW -- "GET /api/route/:stream_id" --> ROUTER
    CM -- "GET /api/route/:stream_id" --> ROUTER
    CT -- "GET /api/route/:stream_id" --> ROUTER
    ROUTER -- "GeoIP lookup" --> DB
    ROUTER -. "nearest edge URL" .-> CW
    ROUTER -. "nearest edge URL" .-> CM
    ROUTER -. "nearest edge URL" .-> CT

    %% ----- Viewer playback -----
    CW == "HLS (.m3u8 / .ts)" ==> E1
    CM == "HLS (.m3u8 / .ts)" ==> E2
    CT == "HLS (.m3u8 / .ts)" ==> E3

    %% ----- Observability -----
    QUIC -. metrics .-> MON
    API  -. metrics .-> MON
    RTMP -. metrics .-> MON
    ROUTER -. metrics .-> MON
    E1 -. metrics .-> MON
    E2 -. metrics .-> MON
    E3 -. metrics .-> MON

    %% ----- Styling -----
    classDef origin   fill:#fde68a,stroke:#b45309,color:#1f2937;
    classDef ingest   fill:#bfdbfe,stroke:#1d4ed8,color:#1f2937;
    classDef control  fill:#ddd6fe,stroke:#6d28d9,color:#1f2937;
    classDef edge     fill:#bbf7d0,stroke:#15803d,color:#1f2937;
    classDef client   fill:#fecaca,stroke:#b91c1c,color:#1f2937;
    classDef store    fill:#e5e7eb,stroke:#374151,color:#1f2937;

    class ST origin;
    class QUIC,FF,RTMP ingest;
    class API,ROUTER,MON control;
    class E1,E2,E3 edge;
    class CW,CM,CT client;
    class DB,HLS store;
```

**Legend**

- **Thick solid arrows (`==>`)** — hot media path (video bytes).
- **Thin solid arrows (`-->`)** — synchronous control / REST calls.
- **Dotted arrows (`-.->`)** — asynchronous responses or telemetry.
- Colors group nodes by tier: origin, ingest, control plane, edge, clients,
  and persistent storage.

## System Overview

```mermaid
graph TB
    subgraph Streamer
        B[Browser<br/>React + WebTransport]
    end

    subgraph Backend
        API[Axum REST API<br/>:8080/tcp]
        QUIC[QUIC Ingest Server<br/>:4443/udp]
        FF[ffmpeg<br/>WebM → H.264/AAC]
    end

    subgraph Storage
        DB[(PostgreSQL<br/>users, streams)]
    end

    subgraph Ingest
        RTMP[nginx-rtmp<br/>:1935]
        HLS[HLS Segments<br/>/tmp/hls]
    end

    subgraph Delivery
        EDGE[Edge Server<br/>nginx :8090]
        ROUTER[Router<br/>GeoIP :8070]
    end

    subgraph Viewer
        VB[Browser<br/>HLS.js Player]
    end

    B -- "QUIC datagrams<br/>(video fragments)" --> QUIC
    B -- "REST<br/>(auth, streams, cert-hash)" --> API
    API -- "SQL" --> DB
    QUIC -- "reassembled chunks<br/>(stdin pipe)" --> FF
    FF -- "RTMP/FLV" --> RTMP
    RTMP -- "writes .m3u8 + .ts" --> HLS
    HLS -- "shared volume (ro)" --> EDGE
    VB -- "GET /hls/*" --> EDGE
    VB -- "GET /api/route/:stream_id" --> ROUTER
    ROUTER -- "nearest edge URL" --> VB
    QUIC -- "validates JWT,<br/>checks stream ownership" --> DB
```

## Request Flows

### Streamer: Go Live

```mermaid
sequenceDiagram
    participant Browser
    participant REST as Backend (REST :8080)
    participant QUIC as Backend (QUIC :4443)
    participant ffmpeg
    participant RTMP as nginx-rtmp

    Browser->>REST: POST /api/auth/login
    REST-->>Browser: JWT token

    Browser->>REST: POST /api/streams {title}
    REST-->>Browser: {id, stream_key}

    Browser->>REST: GET /api/transport/cert-hash
    REST-->>Browser: {sha-256 hash}

    Browser->>QUIC: WebTransport handshake (cert hash + JWT in query)
    QUIC->>QUIC: validate JWT and verify stream ownership
    QUIC-->>Browser: session established

    loop every 250ms
        Browser->>QUIC: datagram fragments (video chunk)
        QUIC->>QUIC: reassemble fragments
        QUIC->>ffmpeg: pipe reassembled WebM chunk
    end

    ffmpeg->>RTMP: RTMP/FLV (H.264 + AAC)
    RTMP->>RTMP: generate HLS .m3u8 + .ts
```

### Viewer: Watch Stream

```mermaid
sequenceDiagram
    participant Viewer as Viewer Browser
    participant Router as Router (:8070)
    participant Edge as Edge Server (:8090)

    Viewer->>Router: GET /api/route/:stream_id
    Router->>Router: GeoIP lookup, pick nearest edge
    Router-->>Viewer: {edge_url}
    Viewer->>Edge: GET /hls/:stream_key/index.m3u8
    Edge-->>Viewer: HLS playlist
    loop playback
        Viewer->>Edge: GET /hls/:stream_key/segmentN.ts
        Edge-->>Viewer: video segment
    end
```

## Container Layout

```mermaid
graph LR
    subgraph docker-compose
        DB[(db<br/>postgres:16)]
        BE[backend<br/>Rust + ffmpeg<br/>:8080 + :4443/udp]
        FE[frontend<br/>React + Vite<br/>:3000]
        MS[ingest-server<br/>nginx-rtmp<br/>:1935 + :8088]
        EDGE[edge<br/>nginx<br/>:8090]
        RT[router<br/>Rust + GeoIP<br/>:8070]
    end

    BE --> DB
    BE -- RTMP --> MS
    MS -- "HLS volume" --> EDGE
    FE --> BE
    RT --> BE
```

## QUIC Datagram Fragment Protocol

Each MediaRecorder chunk (~10–100 KB) is split into QUIC datagrams with an 8-byte header:

```mermaid
graph TD
    CHUNK[MediaRecorder chunk<br/>~50 KB] --> SPLIT{split into<br/>~1200-byte datagrams}
    SPLIT --> D0["Datagram 0<br/>[chunk_id | frag 0/N | payload]"]
    SPLIT --> D1["Datagram 1<br/>[chunk_id | frag 1/N | payload]"]
    SPLIT --> DN["Datagram N<br/>[chunk_id | frag N/N | payload]"]

    RX[Server receives datagrams] --> CHECK{all fragments<br/>for chunk_id?}
    CHECK -- yes --> REASSEMBLE[reassemble in order<br/>and pipe to ffmpeg]
    CHECK -- "no, newer chunk arrived" --> EVICT[evict stale chunk]
    CHECK -- "no, still waiting" --> WAIT[buffer and wait]
```

## Technology Stack

| Layer | Technology |
|-------|-----------|
| Frontend | React, Vite, HLS.js |
| REST API | Rust, Axum, SQLx |
| Video Ingest | QUIC datagrams via wtransport |
| Transcoding | ffmpeg (WebM → H.264/AAC → RTMP) |
| Media Server | nginx-rtmp (RTMP → HLS) |
| Edge Delivery | nginx (static HLS) |
| Geo-Routing | Rust, MaxMind GeoLite2 |
| Database | PostgreSQL 16 |
| Auth | JWT (jsonwebtoken crate) |
