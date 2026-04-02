import React, { useRef, useState, useCallback } from 'react';

const MIME_TYPE = 'video/webm;codecs=vp8,opus';

function ScreenCapture() {
  const videoPreviewRef = useRef(null);
  const mediaRecorderRef = useRef(null);
  const streamRef = useRef(null);
  const chunksRef = useRef([]);

  const [isCapturing, setIsCapturing] = useState(false);
  const [isRecording, setIsRecording] = useState(false);
  const [recordings, setRecordings] = useState([]);
  const [error, setError] = useState(null);

  const startCapture = useCallback(async () => {
    setError(null);
    try {
      const stream = await navigator.mediaDevices.getDisplayMedia({
        video: { cursor: 'always' },
        audio: true,
      });

      streamRef.current = stream;
      if (videoPreviewRef.current) {
        videoPreviewRef.current.srcObject = stream;
      }
      setIsCapturing(true);

      // Handle the user clicking "Stop sharing" in the browser UI
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
  }, []);

  const stopCapture = useCallback(() => {
    if (mediaRecorderRef.current && mediaRecorderRef.current.state !== 'inactive') {
      mediaRecorderRef.current.stop();
    }
    if (streamRef.current) {
      streamRef.current.getTracks().forEach((track) => track.stop());
      streamRef.current = null;
    }
    if (videoPreviewRef.current) {
      videoPreviewRef.current.srcObject = null;
    }
    setIsCapturing(false);
    setIsRecording(false);
  }, []);

  const startRecording = useCallback(() => {
    if (!streamRef.current) return;

    chunksRef.current = [];
    const recorder = new MediaRecorder(streamRef.current, { mimeType: MIME_TYPE });

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

    recorder.start(1000); // collect data every second
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
      if (target) {
        URL.revokeObjectURL(target.url);
      }
      return prev.filter((r) => r.id !== id);
    });
  }, []);

  const deleteAllRecordings = useCallback(() => {
    recordings.forEach((r) => URL.revokeObjectURL(r.url));
    setRecordings([]);
  }, [recordings]);

  return (
    <div style={styles.container}>
      <h2>Screen Capture (Test Mode)</h2>
      <p style={styles.hint}>
        Captures your screen, window, or application tab. Recordings are stored locally in the
        browser for testing — nothing is sent to the server yet.
      </p>

      {error && <p style={styles.error}>{error}</p>}

      {/* Live preview */}
      <div style={styles.previewBox}>
        <video
          ref={videoPreviewRef}
          autoPlay
          muted
          playsInline
          style={styles.video}
        />
        {!isCapturing && (
          <div style={styles.placeholder}>No screen capture active</div>
        )}
      </div>

      {/* Controls */}
      <div style={styles.controls}>
        {!isCapturing ? (
          <button onClick={startCapture} style={styles.btn}>
            Start Capture
          </button>
        ) : (
          <>
            <button onClick={stopCapture} style={{ ...styles.btn, ...styles.btnDanger }}>
              Stop Capture
            </button>
            {!isRecording ? (
              <button onClick={startRecording} style={{ ...styles.btn, ...styles.btnRecord }}>
                Start Recording
              </button>
            ) : (
              <button onClick={stopRecording} style={{ ...styles.btn, ...styles.btnWarn }}>
                Stop Recording
              </button>
            )}
          </>
        )}
      </div>

      {isRecording && <p style={styles.recordingIndicator}>● Recording...</p>}

      {/* Recordings list */}
      {recordings.length > 0 && (
        <div style={styles.recordingsSection}>
          <div style={styles.recordingsHeader}>
            <h3>Recordings ({recordings.length})</h3>
            <button onClick={deleteAllRecordings} style={{ ...styles.btn, ...styles.btnSmall, ...styles.btnDanger }}>
              Delete All
            </button>
          </div>
          {recordings.map((rec) => (
            <div key={rec.id} style={styles.recordingItem}>
              <video src={rec.url} controls style={styles.recordingVideo} />
              <div style={styles.recordingInfo}>
                <span>{rec.timestamp}</span>
                <span>{(rec.blob.size / (1024 * 1024)).toFixed(1)} MB</span>
                <button
                  onClick={() => deleteRecording(rec.id)}
                  style={{ ...styles.btn, ...styles.btnSmall, ...styles.btnDanger }}
                >
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

const styles = {
  container: {
    maxWidth: 800,
    margin: '0 auto',
    padding: 20,
    fontFamily: 'system-ui, sans-serif',
  },
  hint: {
    color: '#666',
    fontSize: 14,
    marginBottom: 16,
  },
  error: {
    color: '#d32f2f',
    background: '#fdecea',
    padding: '8px 12px',
    borderRadius: 4,
    marginBottom: 12,
  },
  previewBox: {
    position: 'relative',
    background: '#000',
    borderRadius: 8,
    overflow: 'hidden',
    aspectRatio: '16/9',
    marginBottom: 16,
  },
  video: {
    width: '100%',
    height: '100%',
    objectFit: 'contain',
    display: 'block',
  },
  placeholder: {
    position: 'absolute',
    inset: 0,
    display: 'flex',
    alignItems: 'center',
    justifyContent: 'center',
    color: '#666',
    fontSize: 18,
  },
  controls: {
    display: 'flex',
    gap: 10,
    marginBottom: 12,
  },
  btn: {
    padding: '10px 20px',
    border: 'none',
    borderRadius: 6,
    fontSize: 14,
    fontWeight: 600,
    cursor: 'pointer',
    background: '#1976d2',
    color: '#fff',
  },
  btnDanger: {
    background: '#d32f2f',
  },
  btnRecord: {
    background: '#388e3c',
  },
  btnWarn: {
    background: '#f57c00',
  },
  btnSmall: {
    padding: '6px 12px',
    fontSize: 12,
  },
  recordingIndicator: {
    color: '#d32f2f',
    fontWeight: 700,
    fontSize: 16,
    animation: 'pulse 1s infinite',
  },
  recordingsSection: {
    marginTop: 24,
    borderTop: '1px solid #ddd',
    paddingTop: 16,
  },
  recordingsHeader: {
    display: 'flex',
    justifyContent: 'space-between',
    alignItems: 'center',
    marginBottom: 12,
  },
  recordingItem: {
    background: '#f5f5f5',
    borderRadius: 8,
    padding: 12,
    marginBottom: 12,
  },
  recordingVideo: {
    width: '100%',
    borderRadius: 4,
    marginBottom: 8,
  },
  recordingInfo: {
    display: 'flex',
    justifyContent: 'space-between',
    alignItems: 'center',
    fontSize: 13,
    color: '#555',
  },
};

export default ScreenCapture;
