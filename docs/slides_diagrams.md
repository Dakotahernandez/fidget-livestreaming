# Slide Diagrams — Capture / Encode / Transport / Display

Four high-level diagrams, one per slide. Each shows where data goes,
not how it's implemented.

---

## Slide 1 — Capture

```mermaid
flowchart LR
    SCREEN["Screen"] --> COMP["Compositor"]
    CAM["Webcam"] --> COMP
    MIC["Mic"] --> COMP
    COMP --> OUT(("Media stream"))
    OBS["OBS"] --> OUT
    PHONE["Phone camera"] --> OUT

    classDef src fill:#fde68a,stroke:#b45309,color:#1f2937;
    classDef out fill:#bbf7d0,stroke:#15803d,color:#1f2937;
    class SCREEN,CAM,MIC,OBS,PHONE src;
    class COMP,OUT out;
```

---

## Slide 2 — Encode

```mermaid
flowchart LR
    IN(("Media stream")) --> ENC["Encoder<br/>H.264 + AAC"]
    ENC --> LAD["Rendition ladder<br/>source · 720p · 360p"]
    LAD --> OUT(("RTMP"))

    classDef in fill:#fde68a,stroke:#b45309,color:#1f2937;
    classDef step fill:#ddd6fe,stroke:#6d28d9,color:#1f2937;
    classDef out fill:#bbf7d0,stroke:#15803d,color:#1f2937;
    class IN in;
    class ENC,LAD step;
    class OUT out;
```

---

## Slide 3 — Transport

```mermaid
flowchart LR
    PUB["Publisher"] -->|"WebTransport / WHIP / RTMP"| ORIGIN["Origin (SRS + backend)"]
    ORIGIN -->|"WebRTC (UDP)"| WHEP["Viewer (live)"]
    ORIGIN --> HLSV[("HLS files")]
    HLSV --> CDN["CDN / edge"]
    CDN -->|"HLS"| HLSP["Viewer (fallback)"]

    classDef pub fill:#fde68a,stroke:#b45309,color:#1f2937;
    classDef ori fill:#ddd6fe,stroke:#6d28d9,color:#1f2937;
    classDef view fill:#bfdbfe,stroke:#1d4ed8,color:#1f2937;
    classDef store fill:#fecaca,stroke:#b91c1c,color:#1f2937;
    classDef del fill:#bbf7d0,stroke:#15803d,color:#1f2937;
    class PUB pub;
    class ORIGIN ori;
    class WHEP,HLSP view;
    class HLSV store;
    class CDN del;
```

---

## Slide 4 — Display

```mermaid
flowchart LR
    NET(("From network")) --> TRY{"Try WHEP"}
    TRY -->|"≤ 1.5 s"| RTC["WebRTC"]
    TRY -->|"timeout"| HLS["HLS"]
    RTC --> VIDEO["&lt;video&gt;"]
    HLS --> VIDEO

    classDef in fill:#fde68a,stroke:#b45309,color:#1f2937;
    classDef step fill:#ddd6fe,stroke:#6d28d9,color:#1f2937;
    classDef out fill:#bbf7d0,stroke:#15803d,color:#1f2937;
    class NET in;
    class TRY,RTC,HLS step;
    class VIDEO out;
```
