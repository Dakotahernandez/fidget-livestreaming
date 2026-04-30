import React, { useRef, useState, useCallback, useEffect } from 'react';
import { getCertHash, getTransportUrl } from '../../services/api';

const MIME_TYPE = 'video/webm;codecs=vp8,opus';
const CANVAS_FPS = 30;
const DRAW_INTERVAL_MS = Math.round(1000 / CANVAS_FPS);
const WEBCAM_SCALE = 0.25;
// Smaller timeslices = each MediaRecorder chunk is dispatched sooner, which
// directly lowers glass-to-glass latency. 100ms keeps WebM clusters small.
const STREAM_TIMESLICE_MS = 100;
const WEBTRANSPORT_UNSUPPORTED_MESSAGE = 'Live streaming requires WebTransport. Safari does not support it yet, so use Chrome or Edge to go live.';

function ScreenCapture({ streamId }) {
  const canvasRef = useRef(null);
  const screenVideoRef = useRef(null);
  const webcamVideoRef = useRef(null);
  const screenStreamRef = useRef(null);
  const webcamStreamRef = useRef(null);
  const compositeStreamRef = useRef(null);
  const drawIntervalRef = useRef(null);

  const [isCapturing, setIsCapturing] = useState(false);
  const [isStreaming, setIsStreaming] = useState(false);
  const [webcamOn, setWebcamOn] = useState(false);
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
      stream.getAudioTracks().forEach((track) => canvasStream.addTrack(track));
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
    compositeStreamRef.current = null;
    if (screenVideoRef.current) screenVideoRef.current.srcObject = null;
    if (webcamVideoRef.current) webcamVideoRef.current.srcObject = null;

    setIsCapturing(false);
    setIsStreaming(false);
    setWebcamOn(false);
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
        mimeType: MIME_TYPE,
      });

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
    </div>
  );
}

export default ScreenCapture;
