import React, { useRef, useState, useCallback, useEffect } from 'react';

const MIME_TYPE = 'video/webm;codecs=vp8,opus';
const CANVAS_FPS = 30;
const DRAW_INTERVAL_MS = Math.round(1000 / CANVAS_FPS);
const WEBCAM_SCALE = 0.25; // webcam occupies 25% of canvas width

function ScreenCapture() {
  const canvasRef = useRef(null);
  const screenVideoRef = useRef(null);
  const webcamVideoRef = useRef(null);
  const mediaRecorderRef = useRef(null);
  const screenStreamRef = useRef(null);
  const webcamStreamRef = useRef(null);
  const compositeStreamRef = useRef(null);
  const drawIntervalRef = useRef(null);
  const chunksRef = useRef([]);

  const [isCapturing, setIsCapturing] = useState(false);
  const [isRecording, setIsRecording] = useState(false);
  const [webcamOn, setWebcamOn] = useState(false);
  const [recordings, setRecordings] = useState([]);
  const [error, setError] = useState(null);

  // Keep a ref so the draw loop always sees the latest webcam state
  const webcamOnRef = useRef(false);
  useEffect(() => { webcamOnRef.current = webcamOn; }, [webcamOn]);

  /** Draws screen (+ optional webcam overlay) onto the canvas once per call. */
  const drawComposite = useCallback(() => {
    const canvas = canvasRef.current;
    const screenVideo = screenVideoRef.current;
    if (!canvas || !screenVideo || screenVideo.readyState < 2) return;

    const ctx = canvas.getContext('2d');
    const sw = screenVideo.videoWidth || 1920;
    const sh = screenVideo.videoHeight || 1080;

    // Match canvas to screen resolution
    if (canvas.width !== sw || canvas.height !== sh) {
      canvas.width = sw;
      canvas.height = sh;
    }

    // Draw screen capture
    ctx.drawImage(screenVideo, 0, 0, sw, sh);

    // Draw webcam overlay in top-right corner
    const webcamVideo = webcamVideoRef.current;
    if (webcamOnRef.current && webcamVideo && webcamVideo.readyState >= 2) {
      const camW = Math.round(sw * WEBCAM_SCALE);
      const camH = Math.round(camW * (webcamVideo.videoHeight / webcamVideo.videoWidth || 0.75));
      const padding = 16;
      const x = sw - camW - padding;
      const y = padding;

      // Rounded rectangle clip + border
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

      // Border
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

      // Feed into a hidden video element for canvas drawing
      const screenVideo = screenVideoRef.current;
      if (screenVideo) {
        screenVideo.srcObject = stream;
        // Explicitly forec play the video element — relying on the autoPlay attribute
        // alone causes a black screen on Windows because hidden video elements
        // may not auto-play reliably on all platforms <----- TODO test on windows again.
        await screenVideo.play();
      }

      // Create composite stream from canvas + screen audio
      const canvas = canvasRef.current;
      const canvasStream = canvas.getContext('2d') && canvas.captureStream(CANVAS_FPS);
      // Carry over audio tracks from screen capture
      stream.getAudioTracks().forEach((track) => canvasStream.addTrack(track));
      compositeStreamRef.current = canvasStream;

      setIsCapturing(true);
      // Use setInterval instead of requestAnimationFrame so the draw loop
      // keeps running even when a permission dialog (getUserMedia) steals focus.
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
    if (mediaRecorderRef.current && mediaRecorderRef.current.state !== 'inactive') {
      mediaRecorderRef.current.stop();
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
    setIsRecording(false);
    setWebcamOn(false);
  }, []);

  const toggleWebcam = useCallback(async () => {
    if (webcamOn) {
      // Turn off
      if (webcamStreamRef.current) {
        webcamStreamRef.current.getTracks().forEach((t) => t.stop());
        webcamStreamRef.current = null;
      }
      if (webcamVideoRef.current) webcamVideoRef.current.srcObject = null;
      setWebcamOn(false);
    } else {
      // Turn on
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
          // Wait for the video to actually start producing frames before
          // flipping the flag, so the draw loop won't try to read a stale element.
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

  const startRecording = useCallback(() => {
    if (!compositeStreamRef.current) return;

    chunksRef.current = [];
    const recorder = new MediaRecorder(compositeStreamRef.current, { mimeType: MIME_TYPE });

    recorder.ondataavailable = (e) => {
      if (e.data.size > 0) {
        chunksRef.current.push(e.data);
      }
    };

    recorder.onstop = () => {
      const blob = new Blob(chunksRef.current, { type: MIME_TYPE });
      const url = URL.createObjectURL(blob);
      const timestamp = new Date().toLocaleTimeString();
      setRecordings((prev) => [...prev, { url, blob, timestamp, id: Date.now() }]);
      chunksRef.current = [];
    };

    recorder.start(1000);
    mediaRecorderRef.current = recorder;
    setIsRecording(true);
  }, []);

  const stopRecording = useCallback(() => {
    if (mediaRecorderRef.current && mediaRecorderRef.current.state !== 'inactive') {
      mediaRecorderRef.current.stop();
    }
    setIsRecording(false);
  }, []);

  const deleteRecording = useCallback((id) => {
    setRecordings((prev) => {
      const target = prev.find((r) => r.id === id);
      if (target) URL.revokeObjectURL(target.url);
      return prev.filter((r) => r.id !== id);
    });
  }, []);

  const deleteAllRecordings = useCallback(() => {
    recordings.forEach((r) => URL.revokeObjectURL(r.url));
    setRecordings([]);
  }, [recordings]);

  // Clean up on unmount
  useEffect(() => {
    return () => {
      if (drawIntervalRef.current) clearInterval(drawIntervalRef.current);
      if (screenStreamRef.current) screenStreamRef.current.getTracks().forEach((t) => t.stop());
      if (webcamStreamRef.current) webcamStreamRef.current.getTracks().forEach((t) => t.stop());
    };
  }, []);

  return (
    <div className="mt-2">
      <h4 style={{ marginBottom: '0.5rem' }}>Screen Capture Preview</h4>
      <p style={{ fontSize: '0.85rem', color: 'var(--color-text-muted)', marginBottom: '0.75rem' }}>
        Test your screen, window, or game capture before going live. Toggle webcam to overlay your
        camera in the top-right corner. Recordings are composited — what you see is what gets recorded.
      </p>

      {error && <p className="error-msg">{error}</p>}

      {/* Hidden video elements that feed into the canvas */}
      <video ref={screenVideoRef} autoPlay muted playsInline style={{ display: 'none' }} />
      <video ref={webcamVideoRef} autoPlay muted playsInline style={{ display: 'none' }} />

      {/* Composited preview canvas */}
      <div className="video-wrapper" style={{ marginBottom: '0.75rem' }}>
        <canvas
          ref={canvasRef}
          style={{ position: 'absolute', top: 0, left: 0, width: '100%', height: '100%' }}
        />
        {!isCapturing && (
          <div className="video-placeholder">No screen capture active</div>
        )}
      </div>

      {/* Controls */}
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
            {!isRecording ? (
              <button className="btn" type="button" onClick={startRecording}
                style={{ background: 'var(--color-success)', color: 'var(--color-text-inverse)' }}>
                Record
              </button>
            ) : (
              <button className="btn" type="button" onClick={stopRecording}
                style={{ background: 'var(--color-warning)', color: 'var(--color-text-inverse)' }}>
                Stop Recording
              </button>
            )}
          </>
        )}
      </div>

      {isRecording && (
        <p style={{ color: 'var(--color-danger)', fontWeight: 700 }}>● Recording...</p>
      )}

      {/* Recordings list */}
      {recordings.length > 0 && (
        <div style={{ borderTop: '1px solid var(--color-border)', paddingTop: '0.75rem', marginTop: '0.75rem' }}>
          <div style={{ display: 'flex', justifyContent: 'space-between', alignItems: 'center', marginBottom: '0.5rem' }}>
            <span style={{ fontSize: '0.9rem', fontWeight: 600 }}>Recordings ({recordings.length})</span>
            <button className="btn btn--danger" type="button" onClick={deleteAllRecordings}
              style={{ padding: '0.3rem 0.75rem', fontSize: '0.8rem' }}>
              Delete All
            </button>
          </div>
          {recordings.map((rec) => (
            <div key={rec.id} style={{
              background: 'var(--color-bg)',
              border: '1px solid var(--color-border)',
              borderRadius: 'var(--radius)',
              padding: '0.75rem',
              marginBottom: '0.5rem',
            }}>
              <video src={rec.url} controls style={{ width: '100%', borderRadius: 'var(--radius)', marginBottom: '0.5rem' }} />
              <div style={{ display: 'flex', justifyContent: 'space-between', alignItems: 'center', fontSize: '0.8rem', color: 'var(--color-text-muted)' }}>
                <span>{rec.timestamp}</span>
                <span>{(rec.blob.size / (1024 * 1024)).toFixed(1)} MB</span>
                <button className="btn btn--danger" type="button" onClick={() => deleteRecording(rec.id)}
                  style={{ padding: '0.25rem 0.6rem', fontSize: '0.75rem' }}>
                  Delete
                </button>
              </div>
            </div>
          ))}
        </div>
      )}
    </div>
  );
}

export default ScreenCapture;
