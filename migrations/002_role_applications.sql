CREATE TABLE IF NOT EXISTS role_applications (
    id UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    user_id UUID NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    target_role VARCHAR(20) NOT NULL DEFAULT 'streamer',
    reason TEXT,
    status VARCHAR(20) NOT NULL DEFAULT 'pending',
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    reviewed_at TIMESTAMPTZ,
    reviewed_by UUID REFERENCES users(id) ON DELETE SET NULL,
    CHECK (target_role IN ('streamer', 'admin')),
    CHECK (status IN ('pending', 'approved', 'denied'))
);

CREATE UNIQUE INDEX IF NOT EXISTS idx_role_applications_one_pending_per_user
ON role_applications(user_id)
WHERE status = 'pending';
