// WHIP  publisher client: cross-browser fallback for streaming. Captures media, sends SDP offer to WHIP endpoint, applies answer, and provides stop() to clean up and notify SRS.

const ICE_GATHERING_TIMEOUT_MS = 3000;

// Wait for ICE gathering to finish before sending SDP offer. Falls back to partial candidates after timeout; SRS usually accepts these.
function waitForIceGathering(pc) {
  if (pc.iceGatheringState === 'complete') {
    return Promise.resolve();
  }

  return new Promise((resolve) => {
    let settled = false;
    const finish = () => {
      if (settled) return;
      settled = true;
      pc.removeEventListener('icegatheringstatechange', onChange);
      clearTimeout(timer);
      resolve();
    };
    const onChange = () => {
      if (pc.iceGatheringState === 'complete') {
        finish();
      }
    };
    pc.addEventListener('icegatheringstatechange', onChange);
    const timer = setTimeout(finish, ICE_GATHERING_TIMEOUT_MS);
  });
}

// Normalize SRS WHIP Location header to an absolute resource URL for later DELETE.
function resolveResourceUrl(locationHeader, requestUrl) {
  if (!locationHeader) return null;

  try {
    return new URL(locationHeader, window.location.origin).toString();
  } catch {
    try {
      return new URL(locationHeader, requestUrl).toString();
    } catch {
      return null;
    }
  }
}

// Start a WHIP publish session. Resolves after SRS accepts SDP answer. Media flows once ICE is connected. Throws on failure.
export async function startWhipPublish({ whipUrl, mediaStream }) {
  if (!whipUrl) {
    throw new Error('WHIP publish URL is missing');
  }
  if (!mediaStream) {
    throw new Error('No media stream supplied to WHIP publisher');
  }

  const pc = new RTCPeerConnection({ bundlePolicy: 'max-bundle' });

  // Add each track as sendonly transceiver for publish-only SDP.
  for (const track of mediaStream.getTracks()) {
    pc.addTransceiver(track, {
      direction: 'sendonly',
      streams: [mediaStream],
    });
  }

  let resourceUrl = null;

  try {
    const offer = await pc.createOffer();
    await pc.setLocalDescription(offer);
    await waitForIceGathering(pc);

    const localSdp = pc.localDescription?.sdp;
    if (!localSdp) {
      throw new Error('Failed to generate local SDP offer');
    }

    const response = await fetch(whipUrl, {
      method: 'POST',
      headers: { 'Content-Type': 'application/sdp' },
      body: localSdp,
    });

    if (!response.ok) {
      // SRS surfaces RtcStreamBusy as 5xx when a previous publisher
      // session for the same stream key is still alive (typically a
      // stale ICE/DTLS session left over from a tab reload). Give the
      // user an actionable hint instead of a bare HTTP code.
      if (response.status === 502 || response.status === 503) {
        throw new Error(
          `WHIP signalling failed: ${response.status} (stream is busy — wait ~30s for the previous session to time out, then click Go Live again)`
        );
      }
      throw new Error(`WHIP signalling failed: ${response.status}`);
    }

    resourceUrl = resolveResourceUrl(response.headers.get('Location'), whipUrl);
    const answerSdp = await response.text();
    if (!answerSdp) {
      throw new Error('WHIP server returned an empty answer');
    }

    await pc.setRemoteDescription({ type: 'answer', sdp: answerSdp });
  } catch (err) {
    try { pc.close(); } catch { /* ignore */ }
    throw err;
  }

  // Tear down connection and notify SRS with DELETE. Best effort; SRS also detects ICE drop.
  const stop = async () => {
    try {
      pc.getSenders().forEach((sender) => {
        try { sender.track?.stop(); } catch { /* ignore */ }
      });
    } catch { /* ignore */ }

    try { pc.close(); } catch { /* ignore */ }

    if (resourceUrl) {
      try {
        await fetch(resourceUrl, { method: 'DELETE' });
      } catch {
        // Best-effort cleanup; on_unpublish fires on ICE timeout too.
      }
    }
  };

  return { peerConnection: pc, resourceUrl, stop };
}
