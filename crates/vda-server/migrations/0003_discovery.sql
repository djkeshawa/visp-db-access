CREATE TABLE discovery_sources (
 id uuid PRIMARY KEY,
 provider text NOT NULL CHECK (provider='aws'),
 name text NOT NULL,
 role_arn text,
 external_id_enc text,
 regions text[] NOT NULL,
 default_project_id uuid REFERENCES projects(id) ON DELETE SET NULL,
 environment_tag_keys text[] NOT NULL DEFAULT ARRAY['environment','env','stage'],
 scan_interval_minutes integer NOT NULL DEFAULT 60 CHECK (scan_interval_minutes BETWEEN 5 AND 1440),
 enabled boolean NOT NULL DEFAULT true,
 status text NOT NULL DEFAULT 'idle' CHECK (status IN ('idle','running')),
 created_at timestamptz NOT NULL DEFAULT now(),
 updated_at timestamptz NOT NULL DEFAULT now()
);
CREATE TABLE discovery_runs (
 id uuid PRIMARY KEY,
 source_id uuid NOT NULL REFERENCES discovery_sources(id) ON DELETE CASCADE,
 status text NOT NULL CHECK (status IN ('running','succeeded','partial','failed')),
 started_at timestamptz NOT NULL DEFAULT now(),
 finished_at timestamptz,
 found integer NOT NULL DEFAULT 0,
 new integer NOT NULL DEFAULT 0,
 gone integer NOT NULL DEFAULT 0,
 changed integer NOT NULL DEFAULT 0,
 errors jsonb NOT NULL DEFAULT '[]'
);
CREATE UNIQUE INDEX discovery_one_running ON discovery_runs(source_id) WHERE status='running';
CREATE INDEX discovery_runs_page ON discovery_runs(source_id,started_at DESC,id DESC);
CREATE TABLE discovered_resources (
 id uuid PRIMARY KEY,
 source_id uuid NOT NULL REFERENCES discovery_sources(id) ON DELETE CASCADE,
 arn text NOT NULL,
 region text NOT NULL,
 engine text NOT NULL CHECK (engine IN ('postgres','mysql')),
 payload jsonb NOT NULL,
 tags jsonb NOT NULL DEFAULT '{}',
 status text NOT NULL DEFAULT 'new' CHECK (status IN ('new','imported','ignored','gone')),
 cluster_id uuid REFERENCES clusters(id) ON DELETE SET NULL,
 drift jsonb NOT NULL DEFAULT '[]',
 first_seen_at timestamptz NOT NULL DEFAULT now(),
 last_seen_at timestamptz NOT NULL DEFAULT now(),
 last_run_id uuid REFERENCES discovery_runs(id) ON DELETE SET NULL,
 UNIQUE(source_id,arn)
);
CREATE INDEX discovered_resources_page ON discovered_resources(first_seen_at DESC,id DESC);
CREATE INDEX discovered_resources_filters ON discovered_resources(source_id,status,engine,region);
CREATE INDEX discovery_sources_due ON discovery_sources(enabled) WHERE enabled;
