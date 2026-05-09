-- Stripe Connect: add payout fields to users
ALTER TABLE users
  ADD COLUMN stripe_account_id TEXT,
  ADD COLUMN stripe_onboarding_complete BOOLEAN NOT NULL DEFAULT false;

-- Donation sound library (per streamer, server-backed)
CREATE TABLE donation_sounds (
  id            UUID        PRIMARY KEY DEFAULT gen_random_uuid(),
  streamer_id   UUID        NOT NULL REFERENCES users(id) ON DELETE CASCADE,
  display_name  TEXT        NOT NULL,
  file_path     TEXT        NOT NULL,
  created_at    TIMESTAMPTZ NOT NULL DEFAULT now()
);

-- Donation records
CREATE TABLE donations (
  id                   UUID        PRIMARY KEY DEFAULT gen_random_uuid(),
  stream_id            UUID        NOT NULL REFERENCES streams(id) ON DELETE CASCADE,
  donor_name           TEXT        NOT NULL,
  amount_cents         INT         NOT NULL CHECK (amount_cents >= 100),
  sound_id             UUID        REFERENCES donation_sounds(id) ON DELETE SET NULL,
  stripe_session_id    TEXT        UNIQUE NOT NULL,
  stripe_payment_intent TEXT,
  status               TEXT        NOT NULL DEFAULT 'pending',
  created_at           TIMESTAMPTZ NOT NULL DEFAULT now()
);

CREATE INDEX ON donations(stream_id);
CREATE INDEX ON donations(stripe_session_id);
