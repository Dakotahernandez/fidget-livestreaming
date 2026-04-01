// ============================================
// FIDGET — Void Theme
// Import and use these tokens across your app
// ============================================

export const colors = {
  // Backgrounds
  bg: {
    base:     "#0d0d0d",   // true black — main app background
    surface:  "#1a1a1a",   // cards, panels, nav
    elevated: "#242424",   // modals, dropdowns
    overlay:  "rgba(198, 241, 53, 0.05)", // subtle lime-tinted overlay
  },

  // Brand / Accent
  brand: {
    lime:     "#C6F135",   // primary accent — buttons, highlights, spinner
    green:    "#39FF14",   // electric green — glow effects, active states
    white:    "#ffffff",   // primary text
    offwhite: "#e8ffcc",   // secondary text, wordmark gradient end
  },

  // Text
  text: {
    primary:   "#ffffff",
    secondary: "#a0a0a0",
    muted:     "#555555",
    inverse:   "#0d0d0d",  // text on lime/green backgrounds
  },

  // Borders
  border: {
    subtle: "rgba(198, 241, 53, 0.12)",
    default:"rgba(198, 241, 53, 0.28)",
    strong: "rgba(198, 241, 53, 0.55)",
  },

  // Status
  status: {
    success: "#39FF14",
    warning: "#FFD93D",
    error:   "#FF4444",
    info:    "#60efff",
  },
};

export const gradients = {
  spinner:  "linear-gradient(135deg, #C6F135 0%, #39FF14 100%)",
  wordmark: "linear-gradient(90deg,  #ffffff 0%, #e8ffcc 100%)",
  glow:     "radial-gradient(circle, rgba(198,241,53,0.18) 0%, transparent 70%)",
};

export const shadows = {
  glowSm:  "0 0 8px  rgba(198, 241, 53, 0.45)",
  glowMd:  "0 0 20px rgba(198, 241, 53, 0.35)",
  glowLg:  "0 0 48px rgba(198, 241, 53, 0.20)",
  glowText:"0 0 12px rgba(198, 241, 53, 0.70)",
};

export const animation = {
  spinDuration: "5s",
  spinEasing:   "linear",
  spinIterations: "infinite",
};

// CSS custom properties version — paste into your :root {}
export const cssVariables = `
  --color-bg:            #0d0d0d;
  --color-surface:       #1a1a1a;
  --color-elevated:      #242424;
  --color-brand:         #C6F135;
  --color-brand-green:   #39FF14;
  --color-text:          #ffffff;
  --color-text-secondary:#a0a0a0;
  --color-text-muted:    #555555;
  --color-border:        rgba(198, 241, 53, 0.28);
  --gradient-spinner:    linear-gradient(135deg, #C6F135, #39FF14);
  --gradient-wordmark:   linear-gradient(90deg, #ffffff, #e8ffcc);
  --glow-sm:             0 0 8px  rgba(198, 241, 53, 0.45);
  --glow-md:             0 0 20px rgba(198, 241, 53, 0.35);
  --glow-lg:             0 0 48px rgba(198, 241, 53, 0.20);
`;
