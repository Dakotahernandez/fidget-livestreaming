# Architecture

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
