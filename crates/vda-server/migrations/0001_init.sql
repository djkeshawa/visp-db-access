CREATE TABLE users (
 id uuid PRIMARY KEY, email text NOT NULL UNIQUE CHECK(email=lower(email)), name text NOT NULL,
 password_hash text NOT NULL, org_role text NOT NULL CHECK(org_role IN ('admin','member')),
 disabled boolean NOT NULL DEFAULT false, created_at timestamptz NOT NULL DEFAULT now(), last_login_at timestamptz
);
CREATE TABLE sessions (
 token_hash bytea PRIMARY KEY, user_id uuid NOT NULL REFERENCES users(id) ON DELETE CASCADE,
 expires_at timestamptz NOT NULL, created_at timestamptz NOT NULL DEFAULT now(),
 absolute_expires_at timestamptz NOT NULL DEFAULT now()+interval '7 days', ip inet, user_agent text
);
CREATE INDEX sessions_user ON sessions(user_id);
CREATE INDEX sessions_expiry ON sessions(expires_at);
CREATE TABLE projects (id uuid PRIMARY KEY, name text NOT NULL, description text NOT NULL DEFAULT '', created_at timestamptz NOT NULL DEFAULT now());
CREATE TABLE clusters (
 id uuid PRIMARY KEY, project_id uuid NOT NULL REFERENCES projects(id), name text NOT NULL,
 engine text NOT NULL CHECK(engine IN ('postgres','mysql')),
 provider text NOT NULL CHECK(provider IN ('aws','gcp','azure','onprem','other')), region text NOT NULL,
 environment text NOT NULL CHECK(environment IN ('production','staging','development')),
 host text NOT NULL, port integer NOT NULL CHECK(port BETWEEN 1 AND 65535), database text NOT NULL, username text NOT NULL,
 password_enc text NOT NULL, tls_mode text NOT NULL CHECK(tls_mode IN ('disable','prefer','require','verify_full')),
 replica_host text, replica_port integer CHECK(replica_port BETWEEN 1 AND 65535), tags jsonb NOT NULL DEFAULT '{}',
 created_at timestamptz NOT NULL DEFAULT now(), updated_at timestamptz NOT NULL DEFAULT now()
);
CREATE INDEX clusters_project ON clusters(project_id);
CREATE TABLE cluster_policies (cluster_id uuid PRIMARY KEY REFERENCES clusters(id) ON DELETE CASCADE, policy jsonb NOT NULL);
CREATE TABLE grants (
 id uuid PRIMARY KEY, user_id uuid NOT NULL REFERENCES users(id) ON DELETE CASCADE,
 scope text NOT NULL CHECK(scope IN ('project','cluster')), scope_id uuid NOT NULL,
 level text NOT NULL CHECK(level IN ('read','write','admin')), expires_at timestamptz,
 created_by uuid REFERENCES users(id) ON DELETE SET NULL, created_at timestamptz NOT NULL DEFAULT now()
);
CREATE INDEX grants_user ON grants(user_id,scope,scope_id);
CREATE INDEX grants_scope ON grants(scope,scope_id);
-- Polymorphic scope references cannot use a normal FK. Validate them under row locks,
-- and remove grants with the owning scope in the same transaction.
CREATE FUNCTION validate_grant_scope() RETURNS trigger LANGUAGE plpgsql AS $$
BEGIN
 IF NEW.scope='project' THEN
  PERFORM id FROM projects WHERE id=NEW.scope_id FOR KEY SHARE;
 ELSE
  PERFORM id FROM clusters WHERE id=NEW.scope_id FOR KEY SHARE;
 END IF;
 IF NOT FOUND THEN RAISE EXCEPTION 'grant scope does not exist' USING ERRCODE='23503'; END IF;
 RETURN NEW;
END $$;
CREATE TRIGGER grant_scope_fk BEFORE INSERT OR UPDATE ON grants FOR EACH ROW EXECUTE FUNCTION validate_grant_scope();
CREATE TABLE approvals (
 id uuid PRIMARY KEY, cluster_id uuid NOT NULL REFERENCES clusters(id) ON DELETE CASCADE,
 requester_id uuid NOT NULL REFERENCES users(id), sql text NOT NULL, reason text NOT NULL, analysis jsonb NOT NULL,
 status text NOT NULL CHECK(status IN ('pending','approved','rejected','executing','executed','failed','expired')),
 reviewer_id uuid REFERENCES users(id), review_note text, result jsonb,
 created_at timestamptz NOT NULL DEFAULT now(), reviewed_at timestamptz, executed_at timestamptz,
 expires_at timestamptz NOT NULL DEFAULT now()+interval '24 hours',
 CHECK(reviewer_id IS NULL OR reviewer_id<>requester_id)
);
CREATE INDEX approvals_cluster_status ON approvals(cluster_id,status,created_at);
CREATE TABLE query_history (
 id uuid PRIMARY KEY, cluster_id uuid NOT NULL, cluster_ref uuid REFERENCES clusters(id) ON DELETE SET NULL, user_id uuid NOT NULL REFERENCES users(id),
 cluster_name text NOT NULL, user_email text NOT NULL, sql text NOT NULL,
 verdict text NOT NULL CHECK(verdict IN ('allow','requires_approval','deny')),
 status text NOT NULL CHECK(status IN ('ok','blocked','error','cancelled')), row_count bigint, elapsed_ms bigint,
 error text, created_at timestamptz NOT NULL DEFAULT now()
);
CREATE INDEX history_cluster_time ON query_history(cluster_id,created_at DESC,id DESC);
CREATE INDEX history_user_time ON query_history(user_id,created_at DESC,id DESC);
CREATE INDEX history_time ON query_history(created_at DESC,id DESC);
CREATE TABLE health_checks (
 id bigserial PRIMARY KEY, cluster_id uuid NOT NULL REFERENCES clusters(id) ON DELETE CASCADE,
 endpoint text NOT NULL DEFAULT 'primary' CHECK(endpoint IN ('primary','replica')),
 status text NOT NULL CHECK(status IN ('healthy','degraded','down','unknown')), latency_ms bigint,
 server_version text, is_replica boolean, details jsonb NOT NULL DEFAULT '{}', checked_at timestamptz NOT NULL DEFAULT now()
);
CREATE INDEX health_cluster_time ON health_checks(cluster_id,endpoint,checked_at DESC);
CREATE TABLE audit_log (
 id uuid PRIMARY KEY, actor_id uuid REFERENCES users(id), action text NOT NULL, target_type text, target_id uuid,
 ip inet, details jsonb NOT NULL DEFAULT '{}', created_at timestamptz NOT NULL DEFAULT now()
);
CREATE INDEX audit_time ON audit_log(created_at DESC,id DESC);
CREATE FUNCTION audit_append_only() RETURNS trigger LANGUAGE plpgsql AS $$
BEGIN RAISE EXCEPTION 'audit_log is append-only'; END $$;
CREATE TRIGGER audit_immutable BEFORE UPDATE OR DELETE OR TRUNCATE ON audit_log FOR EACH STATEMENT EXECUTE FUNCTION audit_append_only();
CREATE TABLE settings (key text PRIMARY KEY, value jsonb NOT NULL);
INSERT INTO settings(key,value) VALUES ('network','{"allowed_cidrs":[],"trust_proxy_headers":false}');
