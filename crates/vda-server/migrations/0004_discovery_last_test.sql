-- Connection test summaries contain no credentials and survive gateway restarts.
ALTER TABLE discovery_sources ADD COLUMN last_test jsonb;
