-- Never change the original migration: existing installations upgrade in place.
ALTER TABLE query_history DROP CONSTRAINT query_history_status_check;
ALTER TABLE query_history ADD CONSTRAINT query_history_status_check
 CHECK (status IN ('running','unknown','ok','blocked','error','cancelled'));
ALTER TABLE query_history ADD COLUMN ip inet;
ALTER TABLE approvals ADD COLUMN error text;
ALTER TABLE approvals ADD COLUMN execution_started_at timestamptz;
ALTER TABLE approvals ADD COLUMN execution_timeout_ms bigint NOT NULL DEFAULT 600000;
CREATE INDEX approvals_page ON approvals(created_at DESC,id DESC);
CREATE INDEX history_running ON query_history(created_at) WHERE status='running';
