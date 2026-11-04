ALTER TABLE memberships DROP CONSTRAINT memberships_role_check;
ALTER TABLE memberships ADD CONSTRAINT memberships_role_check CHECK (role IN ('owner','interviewer','reviewer'));
ALTER TABLE audit_events ALTER COLUMN actor_id DROP NOT NULL;
ALTER TABLE audit_events ADD COLUMN entity_id uuid;
ALTER TABLE audit_events ADD COLUMN metadata jsonb NOT NULL DEFAULT '{}';
CREATE INDEX audit_timeline ON audit_events(organization_id, created_at DESC, id DESC);

CREATE TABLE jobs (
    id uuid PRIMARY KEY,
    organization_id uuid NOT NULL REFERENCES organizations(id),
    request_key uuid NOT NULL,
    title text NOT NULL CHECK (length(title) BETWEEN 1 AND 160),
    created_by uuid NOT NULL REFERENCES users(id),
    created_at timestamptz NOT NULL DEFAULT now(),
    UNIQUE(organization_id,id), UNIQUE(organization_id,request_key)
);
CREATE TABLE job_description_versions (
    id uuid PRIMARY KEY,
    organization_id uuid NOT NULL,
    job_id uuid NOT NULL,
    version integer NOT NULL CHECK (version > 0),
    description text NOT NULL CHECK(length(description) BETWEEN 20 AND 20000),
    created_at timestamptz NOT NULL DEFAULT now(),
    FOREIGN KEY(organization_id,job_id) REFERENCES jobs(organization_id,id),
    UNIQUE(organization_id,id), UNIQUE(organization_id,job_id,id), UNIQUE(job_id,version)
);
CREATE TABLE competency_blueprints (
    id uuid PRIMARY KEY,
    organization_id uuid NOT NULL,
    jd_version_id uuid NOT NULL,
    content jsonb NOT NULL CHECK(jsonb_typeof(content)='object'),
    created_at timestamptz NOT NULL DEFAULT now(),
    FOREIGN KEY(organization_id,jd_version_id) REFERENCES job_description_versions(organization_id,id),
    UNIQUE(organization_id,id)
);
CREATE TABLE interview_sessions (
    id uuid PRIMARY KEY,
    organization_id uuid NOT NULL,
    job_id uuid NOT NULL,
    jd_version_id uuid NOT NULL,
    request_key uuid NOT NULL,
    candidate_name text NOT NULL CHECK(length(candidate_name) BETWEEN 1 AND 120),
    candidate_email text NOT NULL CHECK(length(candidate_email) <= 254 AND candidate_email=lower(candidate_email)),
    state text NOT NULL DEFAULT 'invited' CHECK(state IN ('invited','ready','in_progress','submitted','scoring','completed','expired')),
    duration_minutes integer NOT NULL CHECK(duration_minutes BETWEEN 5 AND 180),
    expires_at timestamptz NOT NULL DEFAULT now() + interval '7 days',
    started_at timestamptz,
    deadline_at timestamptz,
    submitted_at timestamptz,
    created_by uuid NOT NULL REFERENCES users(id),
    created_at timestamptz NOT NULL DEFAULT now(),
    FOREIGN KEY(organization_id,job_id,jd_version_id) REFERENCES job_description_versions(organization_id,job_id,id),
    UNIQUE(organization_id,id), UNIQUE(organization_id,request_key)
);
CREATE INDEX sessions_timeline ON interview_sessions(organization_id,created_at DESC,id DESC);
-- Capability lookup is global, like auth_sessions. Only hashed secrets and routing IDs live here.
CREATE TABLE candidate_access (
    token_hash text PRIMARY KEY,
    organization_id uuid NOT NULL,
    session_id uuid NOT NULL,
    expires_at timestamptz NOT NULL,
    FOREIGN KEY(organization_id,session_id) REFERENCES interview_sessions(organization_id,id)
);
CREATE TABLE interview_questions (
    id uuid PRIMARY KEY,
    organization_id uuid NOT NULL,
    session_id uuid NOT NULL,
    position integer NOT NULL CHECK(position > 0),
    kind text NOT NULL CHECK(kind IN ('text','code','scenario')),
    difficulty text NOT NULL CHECK(difficulty IN ('easy','medium','hard')),
    prompt text NOT NULL,
    rubric jsonb NOT NULL CHECK(jsonb_typeof(rubric)='object'),
    served_at timestamptz,
    FOREIGN KEY(organization_id,session_id) REFERENCES interview_sessions(organization_id,id),
    UNIQUE(organization_id,session_id,id), UNIQUE(session_id,position)
);
CREATE TABLE interview_answers (
    id uuid PRIMARY KEY,
    organization_id uuid NOT NULL,
    session_id uuid NOT NULL,
    question_id uuid NOT NULL,
    content text NOT NULL DEFAULT '',
    revision integer NOT NULL DEFAULT 1 CHECK(revision > 0),
    updated_at timestamptz NOT NULL DEFAULT now(),
    FOREIGN KEY(organization_id,session_id,question_id) REFERENCES interview_questions(organization_id,session_id,id),
    UNIQUE(session_id,question_id)
);
-- Global operational queue: no candidate content. Worker enters tenant scope before domain access.
CREATE TABLE background_jobs (
    id uuid PRIMARY KEY,
    organization_id uuid NOT NULL REFERENCES organizations(id),
    session_id uuid NOT NULL,
    kind text NOT NULL CHECK(kind='expire_session'),
    available_at timestamptz NOT NULL,
    attempts integer NOT NULL DEFAULT 0,
    status text NOT NULL DEFAULT 'pending' CHECK(status IN ('pending','completed','failed')),
    last_error text,
    created_at timestamptz NOT NULL DEFAULT now(),
    FOREIGN KEY(organization_id,session_id) REFERENCES interview_sessions(organization_id,id),
    UNIQUE(session_id,kind)
);
CREATE INDEX background_jobs_due ON background_jobs(available_at) WHERE status='pending';

DO $$
DECLARE table_name text;
BEGIN
    FOREACH table_name IN ARRAY ARRAY['jobs','job_description_versions','competency_blueprints','interview_sessions','interview_questions','interview_answers'] LOOP
        EXECUTE format('ALTER TABLE %I ENABLE ROW LEVEL SECURITY',table_name);
        EXECUTE format('ALTER TABLE %I FORCE ROW LEVEL SECURITY',table_name);
        EXECUTE format('CREATE POLICY tenant_scope ON %I USING (organization_id = nullif(current_setting(''app.tenant_id'',true),'''')::uuid) WITH CHECK (organization_id = nullif(current_setting(''app.tenant_id'',true),'''')::uuid)',table_name);
    END LOOP;
END $$;
GRANT SELECT,INSERT ON jobs,job_description_versions,competency_blueprints,interview_questions TO platform_app;
GRANT SELECT,INSERT,UPDATE ON interview_sessions,interview_answers TO platform_app;
GRANT SELECT,INSERT ON candidate_access TO platform_app;
GRANT SELECT,INSERT,UPDATE ON background_jobs TO platform_app;
