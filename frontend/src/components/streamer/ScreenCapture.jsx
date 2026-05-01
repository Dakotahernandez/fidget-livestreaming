import React, { useRef, useState, useCallback, useEffect } from 'react';
import { getCertHash, getTransportUrl } from '../../services/api';

// Preferred ingest codec is H.264/AAC inside fragmented MP4 — the backend
// can `-c:v copy` it straight to RTMP, skipping the libx264 transcode
// that adds 200-400ms of glass-to-glass latency. We fall back to VP8/Opus
// in WebM on browsers that don't yet support fMP4 from MediaRecorder
// (Chrome < 130, Firefox).
const PREFERRED_MIME_TYPES = [
  'video/mp4;codecs=avc1.42E01F,mp4a.40.2',
  'video/webm;codecs=vp8,opus',
];
const CANVAS_FPS = 30;
const DRAW_INTERVAL_MS = Math.round(1000 / CANVAS_FPS);
const WEBCAM_SCALE = 0.25;
// Smaller timeslices = each MediaRecorder chunk is dispatched sooner, which
// directly lowers glass-to-glass latency. 100ms keeps WebM clusters small.
const STREAM_TIMESLICE_MS = 100;
const WEBTRANSPORT_UNSUPPORTED_MESSAGE = 'Live streaming requires WebTransport. Safari does not support it yet, so use Chrome or Edge to go live.';

/// Pick the lowest-latency MediaRecorder mime type the current browser
/// actually supports.
function pickRecorderMimeType() {
  if (typeof MediaRecorder === 'undefined') return null;
  for (const mime of PREFERRED_MIME_TYPES) {
    try {
      if (MediaRecorder.isTypeSupported(mime)) return mime;
    } catch { /* ignore */ }
  }
  return null;
}

function ScreenCapture({ streamId }) {
  const canvasRef = useRef(null);
  const screenVideoRef = useRef(null);
  const webcamVideoRef = useRef(null);
  const screenStreamRef = useRef(null);
  const webcamStreamRef = useRef(null);
  const micStreamRef = useRef(null);
  const compositeStreamRef = useRef(null);
  const drawIntervalRef = useRef(null);

  // Web Audio mixer: a single AudioContext + MediaStreamDestination
  // combines screen audio, microphone, and soundboard playback into one
  // audio track. MediaRecorder reliably encodes only the first audio track
  // in a MediaStream, so we feed it our mixed track instead of trying to
  // attach mic/screen/soundboard tracks separately.
  const audioCtxRef = useRef(null);
  const audioDestRef = useRef(null);
  const screenAudioSrcRef = useRef(null);
  const micAudioSrcRef = useRef(null);
  const soundBuffersRef = useRef(new Map());

  const [isCapturing, setIsCapturing] = useState(false);
  const [isStreaming, setIsStreaming] = useState(false);
  const [webcamOn, setWebcamOn] = useState(false);
  const [micOn, setMicOn] = useState(false);
  const [customSounds, setCustomSounds] = useState([]);
  const [error, setError] = useState(null);

  const wsRef = useRef(null);
  const transportRef = useRef(null);
  const streamWriterRef = useRef(null);
  const streamRecorderRef = useRef(null);

  const webcamOnRef = useRef(false);
  useEffect(() => { webcamOnRef.current = webcamOn; }, [webcamOn]);

  const webTransportSupported = typeof globalThis.WebTransport === 'function';

  const drawComposite = useCallback(() => {
    const canvas = canvasRef.current;
    const screenVideo = screenVideoRef.current;
    if (!canvas || !screenVideo || screenVideo.readyState < 2) return;

    const ctx = canvas.getContext('2d');
    const sw = screenVideo.videoWidth || 1920;
    const sh = screenVideo.videoHeight || 1080;

    if (canvas.width !== sw || canvas.height !== sh) {
      canvas.width = sw;
      canvas.height = sh;
    }

    ctx.drawImage(screenVideo, 0, 0, sw, sh);

    const webcamVideo = webcamVideoRef.current;
    if (webcamOnRef.current && webcamVideo && webcamVideo.readyState >= 2) {
      const camW = Math.round(sw * WEBCAM_SCALE);
      const camH = Math.round(camW * (webcamVideo.videoHeight / webcamVideo.videoWidth || 0.75));
      const padding = 16;
      const x = sw - camW - padding;
      const y = padding;

      const radius = 12;
      ctx.save();
      ctx.beginPath();
      ctx.moveTo(x + radius, y);
      ctx.lineTo(x + camW - radius, y);
      ctx.quadraticCurveTo(x + camW, y, x + camW, y + radius);
      ctx.lineTo(x + camW, y + camH - radius);
      ctx.quadraticCurveTo(x + camW, y + camH, x + camW - radius, y + camH);
      ctx.lineTo(x + radius, y + camH);
      ctx.quadraticCurveTo(x, y + camH, x, y + camH - radius);
      ctx.lineTo(x, y + radius);
      ctx.quadraticCurveTo(x, y, x + radius, y);
      ctx.closePath();
      ctx.clip();
      ctx.drawImage(webcamVideo, x, y, camW, camH);
      ctx.restore();

      ctx.strokeStyle = 'rgba(198, 241, 53, 0.6)';
      ctx.lineWidth = 3;
      ctx.beginPath();
      ctx.moveTo(x + radius, y);
      ctx.lineTo(x + camW - radius, y);
      ctx.quadraticCurveTo(x + camW, y, x + camW, y + radius);
      ctx.lineTo(x + camW, y + camH - radius);
      ctx.quadraticCurveTo(x + camW, y + camH, x + camW - radius, y + camH);
      ctx.lineTo(x + radius, y + camH);
      ctx.quadraticCurveTo(x, y + camH, x, y + camH - radius);
      ctx.lineTo(x, y + radius);
      ctx.quadraticCurveTo(x, y, x + radius, y);
      ctx.closePath();
      ctx.stroke();
    }
  }, []);

  const startCapture = useCallback(async () => {
    setError(null);
    try {
      const stream = await navigator.mediaDevices.getDisplayMedia({
        video: true,
        audio: true,
      });

      screenStreamRef.current = stream;

      const screenVideo = screenVideoRef.current;
      if (screenVideo) {
        screenVideo.srcObject = stream;
        await screenVideo.play();
      }

      const canvas = canvasRef.current;
      const canvasStream = canvas.getContext('2d') && canvas.captureStream(CANVAS_FPS);

      // Build the audio mixer. All audio (screen, mic, soundboard) is
      // routed through `audioDest` so MediaRecorder sees a single mixed
      // track. AudioContext starts suspended in some browsers; resume
      // explicitly so soundboard playback is immediate on first click.
      const AudioCtx = window.AudioContext || window.webkitAudioContext;
      const ctx = new AudioCtx({ sampleRate: 48000 });
      if (ctx.state === 'suspended') {
        try { await ctx.resume(); } catch { /* ignore */ }
      }
      const dest = ctx.createMediaStreamDestination();
      audioCtxRef.current = ctx;
      audioDestRef.current = dest;

      // Pipe shared-screen audio (if the user granted it) into the mixer.
      if (stream.getAudioTracks().length > 0) {
        const screenSrc = ctx.createMediaStreamSource(
          new MediaStream(stream.getAudioTracks())
        );
        screenSrc.connect(dest);
        screenAudioSrcRef.current = screenSrc;
      }

      // Composite stream gets the canvas video track + the mixed audio
      // track. We deliberately do NOT add the raw screen audio tracks here
      // because most browsers' MediaRecorder only encodes the first audio
      // track, so adding multiple causes silent loss of mic/soundboard.
      dest.stream.getAudioTracks().forEach((t) => canvasStream.addTrack(t));
      compositeStreamRef.current = canvasStream;

      setIsCapturing(true);
      drawIntervalRef.current = setInterval(drawComposite, DRAW_INTERVAL_MS);

      stream.getVideoTracks()[0].addEventListener('ended', () => {
        stopCapture();
      });
    } catch (err) {
      if (err.name === 'NotAllowedError') {
        setError('Screen capture permission denied.');
      } else {
        setError(`Failed to start capture: ${err.message}`);
      }
    }
  }, [drawComposite]);

  const stopCapture = useCallback(() => {
    if (drawIntervalRef.current) {
      clearInterval(drawIntervalRef.current);
      drawIntervalRef.current = null;
    }
    if (streamRecorderRef.current && streamRecorderRef.current.state !== 'inactive') {
      streamRecorderRef.current.stop();
    }
    streamRecorderRef.current = null;
    if (streamWriterRef.current) {
      streamWriterRef.current.close().catch(() => {});
      streamWriterRef.current = null;
    }
    if (transportRef.current) {
      transportRef.current.close();
      transportRef.current = null;
    }
    if (wsRef.current) {
      wsRef.current.close();
      wsRef.current = null;
    }
    if (screenStreamRef.current) {
      screenStreamRef.current.getTracks().forEach((t) => t.stop());
      screenStreamRef.current = null;
    }
    if (webcamStreamRef.current) {
      webcamStreamRef.current.getTracks().forEach((t) => t.stop());
      webcamStreamRef.current = null;
    }
    if (micStreamRef.current) {
      micStreamRef.current.getTracks().forEach((t) => t.stop());
      micStreamRef.current = null;
    }
    if (micAudioSrcRef.current) {
      try { micAudioSrcRef.current.disconnect(); } catch { /* ignore */ }
      micAudioSrcRef.current = null;
    }
    if (screenAudioSrcRef.current) {
      try { screenAudioSrcRef.current.disconnect(); } catch { /* ignore */ }
      screenAudioSrcRef.current = null;
    }
    if (audioCtxRef.current) {
      try { audioCtxRef.current.close(); } catch { /* ignore */ }
      audioCtxRef.current = null;
    }
    audioDestRef.current = null;
    compositeStreamRef.current = null;
    if (screenVideoRef.current) screenVideoRef.current.srcObject = null;
    if (webcamVideoRef.current) webcamVideoRef.current.srcObject = null;

    setIsCapturing(false);
    setIsStreaming(false);
    setWebcamOn(false);
    setMicOn(false);
  }, []);

  const toggleWebcam = useCallback(async () => {
    if (webcamOn) {
      if (webcamStreamRef.current) {
        webcamStreamRef.current.getTracks().forEach((t) => t.stop());
        webcamStreamRef.current = null;
      }
      if (webcamVideoRef.current) webcamVideoRef.current.srcObject = null;
      setWebcamOn(false);
    } else {
      setError(null);
      try {
        const camStream = await navigator.mediaDevices.getUserMedia({
          video: { width: 640, height: 480 },
          audio: false,
        });
        webcamStreamRef.current = camStream;
        const webcamVideo = webcamVideoRef.current;
        if (webcamVideo) {
          webcamVideo.srcObject = camStream;
          await webcamVideo.play();
        }
        setWebcamOn(true);
      } catch (err) {
        if (err.name === 'NotAllowedError') {
          setError('Webcam permission denied.');
        } else {
          setError(`Failed to start webcam: ${err.message}`);
        }
      }
    }
  }, [webcamOn]);

  // Toggle the streamer's microphone. The mic feeds the Web Audio mixer
  // (audioDestRef), not the composite stream directly, so it joins screen
  // audio + soundboard in the single mixed track that MediaRecorder
  // encodes. Toggling mid-broadcast works because MediaRecorder keeps
  // recording the mixer's destination track regardless of who is feeding
  // it.
  const toggleMic = useCallback(async () => {
    const ctx = audioCtxRef.current;
    const dest = audioDestRef.current;
    if (micOn) {
      if (micAudioSrcRef.current) {
        try { micAudioSrcRef.current.disconnect(); } catch { /* ignore */ }
        micAudioSrcRef.current = null;
      }
      if (micStreamRef.current) {
        micStreamRef.current.getTracks().forEach((t) => t.stop());
        micStreamRef.current = null;
      }
      setMicOn(false);
      return;
    }

    if (!ctx || !dest) {
      setError('Start screen capture before enabling the microphone.');
      return;
    }

    setError(null);
    try {
      const mic = await navigator.mediaDevices.getUserMedia({
        audio: {
          echoCancellation: true,
          noiseSuppression: true,
          autoGainControl: true,
        },
        video: false,
      });
      micStreamRef.current = mic;
      const src = ctx.createMediaStreamSource(mic);
      src.connect(dest);
      micAudioSrcRef.current = src;
      setMicOn(true);
    } catch (err) {
      if (err.name === 'NotAllowedError') {
        setError('Microphone permission denied.');
      } else {
        setError(`Failed to start microphone: ${err.message}`);
      }
    }
  }, [micOn]);

  // Play a soundboard sample through the audio mixer. The sample plays out
  // both to the streamer's local speakers (so they hear what they triggered)
  // and into the encoded broadcast.
  const playSound = useCallback((id) => {
    const ctx = audioCtxRef.current;
    const dest = audioDestRef.current;
    if (!ctx || !dest) return;
    const buf = soundBuffersRef.current.get(id);
    if (!buf) return;
    if (ctx.state === 'suspended') {
      ctx.resume().catch(() => {});
    }
    const src = ctx.createBufferSource();
    src.buffer = buf;
    // Fan out: mixer (broadcast) AND local speakers (monitor).
    src.connect(dest);
    src.connect(ctx.destination);
    src.start();
  }, []);

  // Decode an uploaded audio file (mp3, wav, ogg, m4a, etc) into an
  // AudioBuffer and add it to the soundboard. The buffer lives only in
  // memory; nothing is uploaded to the server. Audio rides through the
  // same mixer → MediaRecorder → WebTransport → ffmpeg path as the rest
  // of the broadcast, so viewers automatically hear it.
  const addCustomSound = useCallback(async (file) => {
    const ctx = audioCtxRef.current;
    if (!ctx) {
      setError('Start screen capture before adding sounds.');
      return;
    }
    if (!file) return;
    if (file.size > 10 * 1024 * 1024) {
      setError('Sound file is too large (10MB max).');
      return;
    }
    setError(null);
    try {
      const arrayBuf = await file.arrayBuffer();
      // decodeAudioData mutates its input on some browsers; pass a copy.
      const buf = await ctx.decodeAudioData(arrayBuf.slice(0));
      const id = `custom-${Date.now()}-${Math.random().toString(36).slice(2, 8)}`;
      const label = file.name.replace(/\.[^.]+$/, '').slice(0, 24) || 'Custom';
      soundBuffersRef.current.set(id, buf);
      setCustomSounds((prev) => [...prev, { id, label }]);
    } catch (err) {
      setError(`Could not decode "${file.name}": ${err.message || err}`);
    }
  }, []);

  const removeCustomSound = useCallback((id) => {
    soundBuffersRef.current.delete(id);
    setCustomSounds((prev) => prev.filter((s) => s.id !== id));
  }, []);

  const sendChunkOverStream = useCallback(async (arrayBuffer) => {
    const writer = streamWriterRef.current;
    if (!writer) return;

    try {
      await writer.write(new Uint8Array(arrayBuffer));
    } catch (err) {
      console.error('[stream] writer.write failed:', err);
      setError(`Stream send failed: ${err.message || err}. Click Go Live to reconnect.`);
      const t = transportRef.current;
      if (t) {
        try { t.close(); } catch { /* ignore */ }
        transportRef.current = null;
      }
    }
  }, []);

  const startStreaming = useCallback(async () => {
    if (!compositeStreamRef.current || !streamId) return;
    setError(null);

    if (!webTransportSupported) {
      setError(WEBTRANSPORT_UNSUPPORTED_MESSAGE);
      setIsStreaming(false);
      return;
    }

    // Tear down any leftover recorder/transport from a previous session.
    // If we don't, the old MediaRecorder keeps emitting mid-stream WebM
    // clusters (no EBML header) into the new writer, which makes ffmpeg
    // fail with "EBML header parsing failed" on the new session.
    if (streamRecorderRef.current && streamRecorderRef.current.state !== 'inactive') {
      try { streamRecorderRef.current.stop(); } catch { /* ignore */ }
    }
    streamRecorderRef.current = null;
    if (streamWriterRef.current) {
      try { streamWriterRef.current.close(); } catch { /* ignore */ }
      streamWriterRef.current = null;
    }
    if (transportRef.current) {
      try { transportRef.current.close(); } catch { /* ignore */ }
      transportRef.current = null;
    }

    try {
      const { data } = await getCertHash();
      const hashBytes = new Uint8Array(
        data.hash.match(/.{1,2}/g).map((byte) => parseInt(byte, 16)),
      );

      const url = getTransportUrl(streamId);
      const transport = new WebTransport(url, {
        serverCertificateHashes: [
          { algorithm: 'sha-256', value: hashBytes.buffer },
        ],
      });

      await transport.ready;
      transportRef.current = transport;

      // Use a single unidirectional stream instead of datagrams. Datagrams
      // are unreliable and have flaky cross-browser support (Firefox in
      // particular silently drops them on many network paths). A single
      // ordered byte stream gives us reliable in-order WebM delivery and
      // removes the entire fragmentation/reassembly path.
      const wtStream = await transport.createUnidirectionalStream();
      const writer = wtStream.getWriter();
      streamWriterRef.current = writer;
      console.log('[stream] unidirectional stream opened');

      // Prime the stream with a 1-byte write so the QUIC STREAM frame
      // actually goes on the wire immediately. Without this, browsers
      // create the stream object locally but defer the on-wire STREAM frame
      // until the first MediaRecorder chunk arrives (~100ms later), and the
      // backend's accept_uni() can stall if MediaRecorder ever fails to
      // produce data. The byte is a WebM-safe leading zero that ffmpeg's
      // EBML parser ignores before the real header.
      try {
        await writer.write(new Uint8Array([0x00]));
        console.log('[stream] priming byte sent');
      } catch (err) {
        console.error('[stream] failed to send priming byte:', err);
        setError(`Stream priming failed: ${err.message || err}`);
        try { transport.close(); } catch { /* ignore */ }
        transportRef.current = null;
        streamWriterRef.current = null;
        setIsStreaming(false);
        return;
      }

      const recorder = new MediaRecorder(compositeStreamRef.current, {
        mimeType: pickRecorderMimeType() || 'video/webm;codecs=vp8,opus',
      });
      console.log(`[stream] MediaRecorder mime: ${recorder.mimeType}`);

      // Serialize sends so chunk N+1 never beats chunk N to the wire.
      let sendChain = Promise.resolve();
      let chunkCount = 0;
      recorder.ondataavailable = (e) => {
        if (e.data.size > 0 && transportRef.current) {
          sendChain = sendChain.then(() =>
            e.data.arrayBuffer().then((buf) => {
              if (chunkCount === 0) {
                console.log(`[stream] sending first chunk size=${buf.byteLength}`);
              }
              chunkCount += 1;
              return sendChunkOverStream(buf);
            })
          ).catch((err) => {
            console.error('[stream] send chain error:', err);
          });
        }
      };

      recorder.onstop = () => {
        if (streamWriterRef.current) {
          streamWriterRef.current.close().catch(() => {});
          streamWriterRef.current = null;
        }
        if (transportRef.current) {
          transportRef.current.close();
          transportRef.current = null;
        }
      };

      recorder.start(STREAM_TIMESLICE_MS);
      streamRecorderRef.current = recorder;
      setIsStreaming(true);

      const onClosed = () => {
        // Stop the recorder so its still-running ondataavailable handlers
        // don't keep firing into a stale writer when the user clicks Go Live
        // again.
        if (streamRecorderRef.current && streamRecorderRef.current.state !== 'inactive') {
          try { streamRecorderRef.current.stop(); } catch { /* ignore */ }
        }
        streamRecorderRef.current = null;
        streamWriterRef.current = null;
        transportRef.current = null;
        setIsStreaming(false);
      };
      transport.closed.then(onClosed).catch(onClosed);
    } catch (err) {
      setError(`WebTransport connection failed: ${err.message}`);
      setIsStreaming(false);
    }
  }, [streamId, sendChunkOverStream, webTransportSupported]);

  const stopStreaming = useCallback(() => {
    if (streamRecorderRef.current && streamRecorderRef.current.state !== 'inactive') {
      streamRecorderRef.current.stop();
    }
    streamRecorderRef.current = null;
    if (streamWriterRef.current) {
      streamWriterRef.current.close().catch(() => {});
      streamWriterRef.current = null;
    }
    if (transportRef.current) {
      transportRef.current.close();
      transportRef.current = null;
    }
    setIsStreaming(false);
  }, []);

  useEffect(() => {
    return () => {
      if (drawIntervalRef.current) clearInterval(drawIntervalRef.current);
      if (screenStreamRef.current) screenStreamRef.current.getTracks().forEach((t) => t.stop());
      if (webcamStreamRef.current) webcamStreamRef.current.getTracks().forEach((t) => t.stop());
      if (micStreamRef.current) micStreamRef.current.getTracks().forEach((t) => t.stop());
      if (audioCtxRef.current) {
        try { audioCtxRef.current.close(); } catch { /* ignore */ }
      }
      if (streamRecorderRef.current && streamRecorderRef.current.state !== 'inactive') {
        streamRecorderRef.current.stop();
      }
      if (streamWriterRef.current) streamWriterRef.current.close().catch(() => {});
      if (transportRef.current) transportRef.current.close();
      if (wsRef.current) wsRef.current.close();
    };
  }, []);

  return (
    <div className="mt-2">
      <h4 style={{ marginBottom: '0.5rem' }}>Screen Capture Preview</h4>
      <p style={{ fontSize: '0.85rem', color: 'var(--color-text-muted)', marginBottom: '0.75rem' }}>
        Test your screen, window, or game capture before going live. Toggle webcam to overlay your
        camera in the top-right corner.
      </p>

      {error && <p className="error-msg">{error}</p>}
      {!webTransportSupported && (
        <p style={{ fontSize: '0.85rem', color: 'var(--color-warning)', marginBottom: '0.75rem' }}>
          {WEBTRANSPORT_UNSUPPORTED_MESSAGE}
        </p>
      )}

      <video ref={screenVideoRef} autoPlay muted playsInline style={{ display: 'none' }} />
      <video ref={webcamVideoRef} autoPlay muted playsInline style={{ display: 'none' }} />

      <div className="video-wrapper" style={{ marginBottom: '0.75rem' }}>
        <canvas
          ref={canvasRef}
          style={{ position: 'absolute', top: 0, left: 0, width: '100%', height: '100%' }}
        />
        {!isCapturing && (
          <div className="video-placeholder">No screen capture active</div>
        )}
      </div>

      <div style={{ display: 'flex', gap: '0.5rem', flexWrap: 'wrap', marginBottom: '0.75rem' }}>
        {!isCapturing ? (
          <button className="btn" type="button" onClick={startCapture}>
            Start Capture
          </button>
        ) : (
          <>
            <button className="btn btn--danger" type="button" onClick={stopCapture}>
              Stop Capture
            </button>
            <button
              className={`btn ${webcamOn ? 'btn--outline' : ''}`}
              type="button"
              onClick={toggleWebcam}
            >
              {webcamOn ? 'Webcam Off' : 'Webcam On'}
            </button>
            <button
              className={`btn ${micOn ? 'btn--outline' : ''}`}
              type="button"
              onClick={toggleMic}
            >
              {micOn ? 'Mic Off' : 'Mic On'}
            </button>
            {streamId && (              !isStreaming ? (
                <button className="btn" type="button" onClick={startStreaming}
                  disabled={!webTransportSupported}
                  style={{ background: '#e63946', color: '#fff', fontWeight: 700 }}>
                  ● Go Live
                </button>
              ) : (
                <button className="btn" type="button" onClick={stopStreaming}
                  style={{ background: '#e63946', color: '#fff', fontWeight: 700 }}>
                  ■ Stop Live
                </button>
              )
            )}
          </>
        )}
      </div>

      {isStreaming && (
        <p style={{ color: '#e63946', fontWeight: 700 }}>● LIVE — streaming to server</p>
      )}

      {isCapturing && (
        <div className="mt-2" style={{
          padding: '0.75rem',
          border: '1px solid var(--color-border)',
          borderRadius: 'var(--radius)',
        }}>
          <h5 style={{ margin: 0, marginBottom: '0.5rem' }}>Soundboard</h5>
          <p style={{
            fontSize: '0.8rem',
            color: 'var(--color-text-muted)',
            marginTop: 0,
            marginBottom: '0.5rem',
          }}>
            Click a sound to play it through your stream and your local speakers.
          </p>
          <div style={{ display: 'flex', gap: '0.5rem', flexWrap: 'wrap' }}>
            {customSounds.map((s) => (
              <span
                key={s.id}
                style={{ display: 'inline-flex', alignItems: 'center', gap: '0.25rem' }}
              >
                <button
                  type="button"
                  className="btn btn--outline"
                  onClick={() => playSound(s.id)}
                >
                  {s.label}
                </button>
                <button
                  type="button"
                  className="btn btn--outline"
                  onClick={() => removeCustomSound(s.id)}
                  title="Remove sound"
                  style={{ padding: '0.25rem 0.5rem', fontSize: '0.75rem' }}
                >
                  ✕
                </button>
              </span>
            ))}
          </div>

          <div style={{ marginTop: '0.75rem' }}>
            <label
              className="btn btn--outline"
              style={{ cursor: 'pointer', display: 'inline-block' }}
            >
              + Upload sound
              <input
                type="file"
                accept="audio/*"
                multiple
                style={{ display: 'none' }}
                onChange={(e) => {
                  const files = Array.from(e.target.files || []);
                  files.forEach((f) => addCustomSound(f));
                  e.target.value = '';
                }}
              />
            </label>
            <span style={{
              marginLeft: '0.5rem',
              fontSize: '0.75rem',
              color: 'var(--color-text-muted)',
            }}>
              MP3, WAV, OGG, M4A — stored in your browser only.
            </span>
          </div>
        </div>
      )}
    </div>
  );
}

export default ScreenCapture;
