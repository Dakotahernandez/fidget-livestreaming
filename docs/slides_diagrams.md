# Slide Diagrams — Capture / Encode / Transport / Display

One diagram per slide, in pipeline order. Each is self-contained
(legend, color key, and short caption) so you can paste a single
mermaid block per slide without context from neighboring diagrams.

All diagrams render natively in GitHub, VS Code, and any Mermaid
Live exporter (mermaid.live → PNG / SVG) for slide decks.

---

## Slide 1 — Capture

**What it shows:** where the raw pixels and audio originate before
any encoding happens. Three independent capture surfaces feed the
same downstream pipeline.

```mermaid
flowchart LR
    subgraph BROWSER["Browser tab (React app)"]
        direction TB
        SCR["getDisplayMedia()<br/>screen / window / tab"]
        CAM["getUserMedia()<br/>webcam (optional)"]
        MIC["getUserMedia()<br/>microphone"]
        CANVAS["Canvas compositor<br/>screen + webcam overlay<br/>(captureStream())"]
    end

    subgraph OBS_CAP["OBS / external encoder"]
        OBSSRC["Scene sources<br/>display, window, camera, audio devices"]
    end

    subgraph PHONE["Mobile / second device"]
        WHIPCAM["Browser camera<br/>RTCPeerConnection (sendonly)"]
    end

    SCR --> CANVAS
    CAM --> CANVAS
    MIC --> CANVAS

    CANVAS == "MediaStream<br/>(video + audio tracks)" ==> OUT1((to Encode))
    OBSSRC == "MediaStream-equivalent<br/>(internal OBS pipeline)" ==> OUT2((to Encode))
    WHIPCAM == "MediaStream<br/>(camera + mic)" ==> OUT3((to Encode))

    classDef src fill:#fde68a,stroke:#b45309,color:#1f2937;
    classDef comp fill:#bfdbfe,stroke:#1d4ed8,color:#1f2937;
    classDef out fill:#bbf7d0,stroke:#15803d,color:#1f2937;

    class SCR,CAM,MIC,OBSSRC,WHIPCAM src;
    class CANVAS comp;
    class OUT1,OUT2,OUT3 out;
```

**Talking points**

- Browser capture is just W3C Media Capture APIs — no plugins, no
  native code.
- The canvas compositor is what lets us stack a webcam thumbnail on
  top of the shared screen in a single track before we hand the
  stream off to the encoder.
- OBS and a mobile browser are alternate capture surfaces that
  bypass the in-page compositor but converge on the same encode
  stage on the next slide.

---

## Slide 2 — Encode

**What it shows:** how the captured media becomes bytes that can be
shipped over the network. Different capture surfaces use different
encoders (browser-native vs server-side ffmpeg) but all of them end
up as H.264 + AAC in RTMP for the rest of the pipeline.

```mermaid
flowchart LR
    subgraph CAP["From Capture (Slide 1)"]
        MS["MediaStream / OBS scene"]
    end

    subgraph BROWSER_ENC["Browser-side encode"]
        MR["MediaRecorder<br/>VP8 / Opus in WebM<br/>or H.264 / AAC in fMP4<br/>100 ms timeslice"]
        WEBRTC["RTCPeerConnection<br/>VP8 / H.264 + Opus<br/>(WHIP path)"]
    end

    subgraph OBS_ENC["OBS encode"]
        OBSENC["x264 / NVENC<br/>H.264 + AAC<br/>direct RTMP"]
    end

    subgraph BACKEND_ENC["Backend ffmpeg (server-side)"]
        DETECT["detect_input_format()<br/>WebM EBML vs fMP4 ftyp"]
        FF["ffmpeg<br/>libx264 + AAC<br/>-preset veryfast -tune zerolatency<br/>-g 60  (2 s GOP @ 30 fps)"]
        SPLIT["filter_complex split=N<br/>fan-out per rendition"]
        LADDER["Rendition ladder<br/>source / 1080p (copy) / 720p / 360p<br/>per-tier video + audio bitrates"]
    end

    MS --> MR
    MS --> WEBRTC
    MS --> OBSENC

    MR == "WebM / fMP4 byte stream" ==> DETECT
    DETECT --> FF
    FF --> SPLIT
    SPLIT --> LADDER

    WEBRTC == "RTP (H.264 / Opus)" ==> SRSBRIDGE["SRS rtc_to_rtmp bridge"]
    SRSBRIDGE == "RTMP source" ==> WHIPLAD["transcode.rs ladder<br/>720p + 360p<br/>(source = bare key, no re-encode)"]

    OBSENC == "RTMP H.264/AAC" ==> RTMPSRC((to Transport))

    LADDER == "RTMP × N renditions" ==> RTMPSRC
    WHIPLAD == "RTMP × 2 renditions" ==> RTMPSRC

    classDef cap fill:#fde68a,stroke:#b45309,color:#1f2937;
    classDef bx fill:#bfdbfe,stroke:#1d4ed8,color:#1f2937;
    classDef obs fill:#fecaca,stroke:#b91c1c,color:#1f2937;
    classDef be fill:#ddd6fe,stroke:#6d28d9,color:#1f2937;
    classDef out fill:#bbf7d0,stroke:#15803d,color:#1f2937;

    class MS cap;
    class MR,WEBRTC bx;
    class OBSENC obs;
    class DETECT,FF,SPLIT,LADDER,SRSBRIDGE,WHIPLAD be;
    class RTMPSRC out;
```

**Talking points**

- The browser does not encode H.264 itself for the WebTransport
  path — it uses `MediaRecorder`, which produces a self-delimiting
  WebM (VP8/Opus) or fMP4 (H.264/AAC) container. The backend ffmpeg
  re-encodes / fans out into the rendition ladder.
- The WHIP path sends real-time encoded RTP straight from the
  browser; SRS bridges it back to RTMP and the backend's
  `transcode.rs` produces only the lower tiers (the bare source key
  is reused as the Source rendition, no re-encode).
- All three converge on the same RTMP-by-rendition contract for the
  Transport slide.

---

## Slide 3 — Transport

**What it shows:** how encoded bytes traverse the network in both
directions — publisher → origin and origin → viewer. Two ingest
transports (WebTransport / WHIP) and two playback transports
(WHEP / HLS) coexist on the same stream key.

```mermaid
flowchart LR
    subgraph PUB["Publisher transports"]
        WT["WebTransport (QUIC)<br/>:4443/udp<br/>1 ordered unidirectional stream<br/>self-signed + 10-day cert rotation"]
        WHIP["WHIP (WebRTC over HTTPS)<br/>POST /whip/?app=live&stream=&lt;key&gt;<br/>ICE/DTLS/SRTP on UDP :8000"]
        RTMP["RTMP (TCP :1935)<br/>OBS / ffmpeg push"]
    end

    subgraph EDGE_NET["Network boundary"]
        FE["Frontend nginx<br/>same-origin reverse proxy<br/>/api  /router-api  /hls  /whep"]
    end

    subgraph ORIGIN["Origin (EC2)"]
        QUIC["Backend QUIC server<br/>wtransport + JWT validation"]
        SRS["SRS 5<br/>RTMP :1935 - WHIP/WHEP :1985<br/>WebRTC media :8000/udp"]
        HLSVOL[("HLS shared volume<br/>/tmp/hls/hls/&lt;key&gt;/")]
    end

    subgraph VIEWER["Viewer transports"]
        WHEP["WHEP (WebRTC)<br/>POST /whep/<br/>1.5 s first-frame timeout<br/>then fall back"]
        HLS["HLS over HTTPS<br/>.m3u8 + .ts<br/>0.5 s segments / 300 s window"]
    end

    subgraph DELIVERY["Delivery tier"]
        EDGENGINX["Edge nginx<br/>:8090 (local dev)"]
        S3[("AWS S3<br/>HLS origin (prod)")]
        CF["AWS CloudFront<br/>fronts S3 (prod CDN)"]
    end

    WT == "QUIC STREAM frames" ==> QUIC
    QUIC == "stdin pipe" ==> RTMPFAN(["ffmpeg fan-out<br/>(slide 2)"])
    RTMPFAN == "RTMP × N" ==> SRS

    WHIP == "SDP offer (HTTPS)" ==> FE
    FE == "proxy_pass /whep/ /whip/" ==> SRS
    WHIP -. "ICE/DTLS/SRTP UDP 8000" .-> SRS

    RTMP == "RTMP TCP" ==> SRS

    SRS == "writes" ==> HLSVOL
    HLSVOL == "ro mount" ==> EDGENGINX
    HLSVOL == "sync (prod)" ==> S3
    S3 == "origin pull" ==> CF

    WHEP == "POST /whep/" ==> FE
    FE == "proxy" ==> SRS
    SRS -. "WebRTC media UDP 8000" .-> WHEP

    HLS == "GET *.m3u8 / *.ts" ==> EDGENGINX
    HLS == "GET *.m3u8 / *.ts (prod)" ==> CF

    classDef pub fill:#fde68a,stroke:#b45309,color:#1f2937;
    classDef net fill:#e5e7eb,stroke:#374151,color:#1f2937;
    classDef ori fill:#ddd6fe,stroke:#6d28d9,color:#1f2937;
    classDef view fill:#bfdbfe,stroke:#1d4ed8,color:#1f2937;
    classDef del fill:#bbf7d0,stroke:#15803d,color:#1f2937;
    classDef store fill:#fecaca,stroke:#b91c1c,color:#1f2937;

    class WT,WHIP,RTMP pub;
    class FE net;
    class QUIC,SRS,RTMPFAN ori;
    class WHEP,HLS view;
    class EDGENGINX,CF del;
    class HLSVOL,S3 store;
```

**Talking points**

- Three publish transports, two playback transports — the stream
  key is the only thing all of them share.
- Same-origin nginx reverse proxy keeps `/whip/`, `/whep/`, `/hls/`
  and `/api/` on one HTTPS origin so browsers don't trigger CORS
  preflights.
- `VITE_DISABLE_WEBTRANSPORT=1` forces every browser onto the WHIP
  path for deployments behind CDNs that can't proxy UDP.
- The dotted UDP arrows are *out of band* relative to the HTTPS
  signalling — they go straight to the EC2 box on UDP 8000.

---

## Slide 4 — Display

**What it shows:** what the viewer's browser actually does with the
bytes it receives. WHEP first (sub-second), HLS on fallback, with a
manual quality selector that simply rewrites the playback URL.

```mermaid
flowchart LR
    subgraph IN["From Transport (Slide 3)"]
        WHEPIN["WHEP / WebRTC media"]
        HLSIN["HLS playlists + segments"]
    end

    subgraph SELECT["Playback selection"]
        TRY["1) Try WHEP<br/>POST /whep/ (SDP)<br/>wait for first frame ≤ 1.5 s"]
        FB["2) On timeout / fail<br/>fall back to HLS"]
        Q["Quality selector<br/>Source / 720p / 360p<br/>(rewrites stream key suffix)"]
    end

    subgraph PLAYER["Viewer player (Watch.jsx)"]
        VIDEO["&lt;video&gt; element<br/>autoplay muted playsinline"]
        WHEPENG["WebRTC engine<br/>RTCPeerConnection<br/>receive-only transceivers"]
        HLSJS["hls.js (Chrome / FF / Edge)<br/>or native HLS (Safari / iOS)"]
        METRICS["Playback metrics<br/>first-frame latency, ABR rung,<br/>buffer health"]
    end

    subgraph OVERLAY["UI overlay"]
        CHAT["Chat panel<br/>WebTransport bidi stream<br/>WebSocket fallback"]
        QUAL["Quality dropdown"]
        STATS["Stats / debug panel"]
    end

    WHEPIN --> TRY
    HLSIN --> FB

    TRY -- "success" --> WHEPENG
    TRY -- "timeout / error" --> FB
    FB --> HLSJS

    WHEPENG --> VIDEO
    HLSJS --> VIDEO

    Q --> TRY
    Q --> FB
    QUAL --> Q

    VIDEO --> METRICS
    VIDEO --- CHAT
    VIDEO --- STATS

    classDef in fill:#fde68a,stroke:#b45309,color:#1f2937;
    classDef sel fill:#ddd6fe,stroke:#6d28d9,color:#1f2937;
    classDef play fill:#bfdbfe,stroke:#1d4ed8,color:#1f2937;
    classDef ui fill:#bbf7d0,stroke:#15803d,color:#1f2937;

    class WHEPIN,HLSIN in;
    class TRY,FB,Q sel;
    class VIDEO,WHEPENG,HLSJS,METRICS play;
    class CHAT,QUAL,STATS ui;
```

**Talking points**

- WHEP is the happy path: sub-second glass-to-glass on a healthy
  network.
- The 1.5 s first-frame timeout is intentionally aggressive — if
  UDP 8000 is blocked we fall back to HLS in under 2 s instead of
  staring at a black player.
- Quality is a *manual* selector today (no ABR): the dropdown just
  rewrites the stream key (`<key>`, `<key>_720p`, `<key>_360p`) for
  both WHEP and HLS URLs.
- Chat rides a separate WebTransport (or WebSocket fallback)
  channel so it never blocks media decoding.

---

## Appendix — One-line per stage (speaker cheat sheet)

| Slide       | One-line summary                                                               |
|-------------|--------------------------------------------------------------------------------|
| Capture     | Browser `getDisplayMedia` / `getUserMedia` (or OBS) produces a `MediaStream`.  |
| Encode      | `MediaRecorder` / WebRTC / OBS produce containerized media; backend ffmpeg fans out into a rendition ladder. |
| Transport   | WebTransport / WHIP / RTMP in; WHEP and HLS out, all proxied same-origin via nginx. |
| Display     | WHEP first (≤ 1.5 s), HLS fallback, manual quality selector, live chat overlay.|
