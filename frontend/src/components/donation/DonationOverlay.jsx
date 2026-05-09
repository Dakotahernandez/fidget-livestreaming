import React, { useCallback, useEffect, useRef, useState } from 'react';
import { createPortal } from 'react-dom';

/// How long the overlay stays at full opacity before fading.
const VISIBLE_MS = 3000;
/// Fade-out duration. Matches the CSS transition.
const FADE_MS = 300;

/**
 * Hook that owns the donation overlay state and returns a render
 * function plus a `show(alert)` trigger. The overlay is rendered into
 * `document.body` via a portal so it sits above the video player and
 * everything else without inheriting parent stacking contexts.
 */
export function useDonationOverlay() {
  const [current, setCurrent] = useState(null);
  const [fading, setFading] = useState(false);
  const visibleTimerRef = useRef(null);
  const fadeTimerRef = useRef(null);

  // Cleanup any pending timers when the component unmounts so React
  // doesn't warn about state updates on an unmounted component.
  useEffect(() => () => {
    if (visibleTimerRef.current) clearTimeout(visibleTimerRef.current);
    if (fadeTimerRef.current) clearTimeout(fadeTimerRef.current);
  }, []);

  const show = useCallback((alert) => {
    if (visibleTimerRef.current) clearTimeout(visibleTimerRef.current);
    if (fadeTimerRef.current) clearTimeout(fadeTimerRef.current);
    setCurrent(alert);
    setFading(false);

    visibleTimerRef.current = setTimeout(() => {
      setFading(true);
      fadeTimerRef.current = setTimeout(() => {
        setCurrent(null);
        setFading(false);
      }, FADE_MS);
    }, VISIBLE_MS);
  }, []);

  const overlay = current
    ? createPortal(
        <div className={`donation-overlay${fading ? ' donation-overlay--fading' : ''}`}>
          <div className="donation-overlay__title">
            💸 {current.donor_name} donated ${(current.amount_cents / 100).toFixed(2)}
          </div>
        </div>,
        document.body,
      )
    : null;

  return { show, overlay };
}

export default useDonationOverlay;
