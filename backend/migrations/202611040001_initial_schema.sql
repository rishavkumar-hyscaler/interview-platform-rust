-- Development baseline: two user roles, reusable job question sets, eight application tables.
-- Interviewers own jobs; interviewees participate in interview sessions.

CREATE EXTENSION IF NOT EXISTS vector WITH SCHEMA public;

CREATE TABLE users (
    id uuid NOT NULL,
    email text NOT NULL,
    password_hash text,
    created_at timestamptz DEFAULT now() NOT NULL,
    role text DEFAULT 'interviewer'::text NOT NULL,
    CONSTRAINT users_email_check CHECK ((email = lower(email))),
    CONSTRAINT users_password_check CHECK (((role = 'interviewee'::text) OR (password_hash IS NOT NULL))),
    CONSTRAINT users_role_check CHECK ((role = ANY (ARRAY['interviewer'::text, 'interviewee'::text]))),
    CONSTRAINT users_email_role_key UNIQUE (email, role),
    CONSTRAINT users_pkey PRIMARY KEY (id)
);

CREATE TABLE jobs (
    id uuid NOT NULL,
    interviewer_id uuid NOT NULL,
    request_key uuid NOT NULL,
    title text NOT NULL,
    job_code text NOT NULL CHECK (job_code ~ '^[A-Z0-9][A-Z0-9_-]{0,63}$'),
    preparation_duration_minutes integer NOT NULL DEFAULT 30 CHECK (preparation_duration_minutes BETWEEN 5 AND 180),
    question_set_count integer NOT NULL DEFAULT 1 CHECK (question_set_count BETWEEN 1 AND 5),
    CONSTRAINT jobs_interviewer_job_code_key UNIQUE (interviewer_id, job_code),
    created_at timestamptz DEFAULT now() NOT NULL,
    description text NOT NULL,
    blueprint jsonb,
    blueprint_model text,
    blueprint_prompt_version text,
    CONSTRAINT jobs_blueprint_check CHECK (((blueprint IS NULL) OR (jsonb_typeof(blueprint) = 'object'::text))),
    CONSTRAINT jobs_description_check CHECK (((length(description) >= 20) AND (length(description) <= 20000))),
    CONSTRAINT jobs_title_check CHECK (((length(title) >= 1) AND (length(title) <= 160))),
    CONSTRAINT jobs_interviewer_id_id_key UNIQUE (interviewer_id, id),
    CONSTRAINT jobs_interviewer_id_request_key_key UNIQUE (interviewer_id, request_key),
    CONSTRAINT jobs_pkey PRIMARY KEY (id),
    CONSTRAINT jobs_interviewer_id_fkey FOREIGN KEY (interviewer_id) REFERENCES users(id)
);

CREATE TABLE question_sets (
    id uuid PRIMARY KEY,
    interviewer_id uuid NOT NULL,
    job_id uuid NOT NULL,
    duration_minutes integer NOT NULL CHECK (duration_minutes BETWEEN 5 AND 180),
    status text NOT NULL DEFAULT 'pending' CHECK (status IN ('pending','running','completed','failed')),
    seed uuid NOT NULL,
    attempts integer NOT NULL DEFAULT 0 CHECK (attempts >= 0),
    last_error text,
    claim_token uuid,
    lease_until timestamptz,
    available_at timestamptz NOT NULL DEFAULT now(),
    created_at timestamptz NOT NULL DEFAULT now(),
    UNIQUE (interviewer_id,job_id,id),
    FOREIGN KEY (interviewer_id,job_id) REFERENCES jobs(interviewer_id,id)
);

CREATE INDEX question_sets_available ON question_sets(interviewer_id,job_id,duration_minutes,created_at);

CREATE TABLE interview_sessions (
    id uuid NOT NULL,
    interviewer_id uuid NOT NULL,
    job_id uuid NOT NULL,
    request_key uuid NOT NULL,
    question_set_id uuid,
    CONSTRAINT interview_sessions_question_set_fk FOREIGN KEY (interviewer_id, job_id, question_set_id) REFERENCES question_sets(interviewer_id, job_id, id),
    candidate_name text NOT NULL,
    candidate_email text NOT NULL,
    state text DEFAULT 'invited'::text NOT NULL,
    duration_minutes integer NOT NULL,
    expires_at timestamptz DEFAULT (now() + '7 days'::interval) NOT NULL,
    started_at timestamptz,
    deadline_at timestamptz,
    submitted_at timestamptz,
    created_at timestamptz DEFAULT now() NOT NULL,
    interviewee_id uuid NOT NULL,
    CONSTRAINT interview_sessions_candidate_email_check CHECK (((length(candidate_email) <= 254) AND (candidate_email = lower(candidate_email)))),
    CONSTRAINT interview_sessions_candidate_name_check CHECK (((length(candidate_name) >= 1) AND (length(candidate_name) <= 120))),
    CONSTRAINT interview_sessions_duration_minutes_check CHECK (((duration_minutes >= 5) AND (duration_minutes <= 180))),
    CONSTRAINT interview_sessions_state_check CHECK ((state = ANY (ARRAY['invited'::text, 'ready'::text, 'in_progress'::text, 'submitted'::text, 'scoring'::text, 'completed'::text, 'expired'::text]))),
    CONSTRAINT interview_sessions_interviewer_id_id_key UNIQUE (interviewer_id, id),
    CONSTRAINT interview_sessions_interviewer_id_request_key_key UNIQUE (interviewer_id, request_key),
    CONSTRAINT interview_sessions_pkey PRIMARY KEY (id),
    CONSTRAINT interview_sessions_interviewee_id_fkey FOREIGN KEY (interviewee_id) REFERENCES users(id),
    CONSTRAINT interview_sessions_interviewer_id_fkey FOREIGN KEY (interviewer_id) REFERENCES users(id),
    CONSTRAINT interview_sessions_interviewer_id_job_id_fkey FOREIGN KEY (interviewer_id, job_id) REFERENCES jobs(interviewer_id, id)
);

CREATE TABLE auth_sessions (
    token_hash text NOT NULL,
    user_id uuid NOT NULL,
    expires_at timestamptz DEFAULT (now() + '12:00:00'::interval) NOT NULL,
    created_at timestamptz DEFAULT now() NOT NULL,
    interview_session_id uuid,
    interviewer_id uuid,
    sealed_token text,
    CONSTRAINT auth_sessions_check CHECK ((((interview_session_id IS NULL) AND (interviewer_id IS NULL) AND (sealed_token IS NULL)) OR ((interview_session_id IS NOT NULL) AND (interviewer_id IS NOT NULL)))),
    CONSTRAINT auth_sessions_interview_session_id_key UNIQUE (interview_session_id),
    CONSTRAINT auth_sessions_pkey PRIMARY KEY (token_hash),
    CONSTRAINT auth_sessions_interviewer_id_interview_session_id_fkey FOREIGN KEY (interviewer_id, interview_session_id) REFERENCES interview_sessions(interviewer_id, id),
    CONSTRAINT auth_sessions_user_id_fkey FOREIGN KEY (user_id) REFERENCES users(id)
);

CREATE TABLE interview_questions (
    id uuid NOT NULL,
    interviewer_id uuid NOT NULL,
    set_id uuid NOT NULL,
    "position" integer NOT NULL,
    kind text NOT NULL,
    difficulty text NOT NULL,
    prompt text NOT NULL,
    rubric jsonb NOT NULL,
    competency_id text,
    job_id uuid NOT NULL,
    fingerprint text NOT NULL,
    embedding vector,
    embedding_model text,
    embedding_dimensions integer,
    CONSTRAINT interview_questions_difficulty_check CHECK ((difficulty = ANY (ARRAY['easy'::text, 'medium'::text, 'hard'::text]))),
    CONSTRAINT interview_questions_embedding_dimensions_check CHECK (((embedding_dimensions IS NULL) OR ((embedding_dimensions >= 1) AND (embedding_dimensions <= 4096)))),
    CONSTRAINT interview_questions_kind_check CHECK ((kind = ANY (ARRAY['text'::text, 'code'::text, 'scenario'::text]))),
    CONSTRAINT interview_questions_position_check CHECK (("position" > 0)),
    CONSTRAINT interview_questions_rubric_check CHECK ((jsonb_typeof(rubric) = 'object'::text)),
    CONSTRAINT interview_questions_job_id_fingerprint_key UNIQUE (job_id, fingerprint),
    CONSTRAINT interview_questions_set_position_key UNIQUE (set_id, position),
    CONSTRAINT interview_questions_owner_id_key UNIQUE (interviewer_id, id),
    CONSTRAINT interview_questions_set_owner_fk FOREIGN KEY (interviewer_id, job_id, set_id) REFERENCES question_sets(interviewer_id, job_id, id),
    CONSTRAINT interview_questions_pkey PRIMARY KEY (id),
    CONSTRAINT interview_questions_interviewer_id_fkey FOREIGN KEY (interviewer_id) REFERENCES users(id),
    CONSTRAINT interview_questions_interviewer_id_job_id_fkey FOREIGN KEY (interviewer_id, job_id) REFERENCES jobs(interviewer_id, id)
);

CREATE TABLE interview_answers (
    id uuid NOT NULL,
    interviewer_id uuid NOT NULL,
    session_id uuid NOT NULL,
    question_id uuid NOT NULL,
    content text DEFAULT ''::text NOT NULL,
    revision integer DEFAULT 1 NOT NULL,
    updated_at timestamptz DEFAULT now() NOT NULL,
    CONSTRAINT interview_answers_revision_check CHECK ((revision > 0)),
    CONSTRAINT interview_answers_pkey PRIMARY KEY (id),
    CONSTRAINT interview_answers_session_id_question_id_key UNIQUE (session_id, question_id),
    CONSTRAINT interview_answers_question_fk FOREIGN KEY (interviewer_id,question_id) REFERENCES interview_questions(interviewer_id,id),
    CONSTRAINT interview_answers_session_fk FOREIGN KEY (interviewer_id,session_id) REFERENCES interview_sessions(interviewer_id,id),
    CONSTRAINT interview_answers_interviewer_id_fkey FOREIGN KEY (interviewer_id) REFERENCES users(id)
);

CREATE TABLE audit_events (
    id uuid NOT NULL,
    interviewer_id uuid NOT NULL,
    actor_id uuid,
    action text NOT NULL,
    created_at timestamptz DEFAULT now() NOT NULL,
    entity_id uuid,
    metadata jsonb DEFAULT '{}'::jsonb NOT NULL,
    CONSTRAINT audit_events_pkey PRIMARY KEY (id),
    CONSTRAINT audit_events_actor_id_fkey FOREIGN KEY (actor_id) REFERENCES users(id),
    CONSTRAINT audit_events_interviewer_id_fkey FOREIGN KEY (interviewer_id) REFERENCES users(id)
);

CREATE INDEX audit_timeline ON audit_events USING btree (interviewer_id, created_at DESC, id DESC);

CREATE INDEX auth_sessions_expiry ON auth_sessions USING btree (expires_at);

CREATE INDEX questions_job ON interview_questions USING btree (interviewer_id, job_id, embedding_model, embedding_dimensions);


CREATE INDEX sessions_timeline ON interview_sessions USING btree (interviewer_id, created_at DESC, id DESC);

-- Transaction-local ownership is derived from authenticated interviewer identity.
DO $$
DECLARE table_name text;
BEGIN
    FOREACH table_name IN ARRAY ARRAY[
        'question_sets','jobs','interview_sessions','interview_questions','interview_answers','audit_events'
    ] LOOP
        EXECUTE format('ALTER TABLE %I ENABLE ROW LEVEL SECURITY',table_name);
        EXECUTE format('ALTER TABLE %I FORCE ROW LEVEL SECURITY',table_name);
        EXECUTE format($policy$CREATE POLICY interviewer_scope ON %I USING (interviewer_id = nullif(current_setting('app.interviewer_id',true), '')::uuid) WITH CHECK (interviewer_id = nullif(current_setting('app.interviewer_id',true), '')::uuid)$policy$,table_name);
    END LOOP;
END $$;

GRANT USAGE ON SCHEMA public TO platform_app;

GRANT SELECT,INSERT ON TABLE audit_events TO platform_app;

GRANT SELECT,INSERT,DELETE ON TABLE auth_sessions TO platform_app;

GRANT UPDATE(token_hash) ON TABLE auth_sessions TO platform_app;

GRANT UPDATE(sealed_token) ON TABLE auth_sessions TO platform_app;

GRANT SELECT,INSERT,UPDATE ON TABLE interview_answers TO platform_app;

GRANT SELECT,INSERT ON TABLE interview_questions TO platform_app;

GRANT SELECT,INSERT,UPDATE ON TABLE interview_sessions TO platform_app;

GRANT SELECT,INSERT ON TABLE jobs TO platform_app;

GRANT UPDATE(blueprint) ON TABLE jobs TO platform_app;

GRANT UPDATE(blueprint_model) ON TABLE jobs TO platform_app;

GRANT UPDATE(blueprint_prompt_version) ON TABLE jobs TO platform_app;

GRANT SELECT,INSERT ON TABLE users TO platform_app;

CREATE INDEX interview_sessions_question_set ON interview_sessions(question_set_id) WHERE question_set_id IS NOT NULL;
GRANT SELECT,INSERT,UPDATE ON TABLE question_sets TO platform_app;

-- Project reusable questions into each assigned interview without copying rows.
CREATE FUNCTION session_question_details() RETURNS TABLE (
    id uuid,interviewer_id uuid,session_id uuid,"position" integer,kind text,difficulty text,
    prompt text,rubric jsonb,competency_id text,job_id uuid,fingerprint text,
    embedding vector,embedding_model text,embedding_dimensions integer,set_id uuid
) LANGUAGE sql STABLE SECURITY INVOKER AS $$
SELECT q.id,q.interviewer_id,s.id,q.position,q.kind,q.difficulty,q.prompt,q.rubric,
       q.competency_id,q.job_id,q.fingerprint,q.embedding,q.embedding_model,q.embedding_dimensions,q.set_id
FROM interview_sessions s JOIN interview_questions q
  ON q.interviewer_id=s.interviewer_id AND q.set_id=s.question_set_id;
$$;
GRANT EXECUTE ON FUNCTION session_question_details() TO platform_app;

CREATE FUNCTION check_answer_question_membership() RETURNS trigger LANGUAGE plpgsql AS $$
BEGIN
    IF NOT EXISTS (SELECT 1 FROM session_question_details()
                   WHERE interviewer_id=NEW.interviewer_id AND session_id=NEW.session_id AND id=NEW.question_id) THEN
        RAISE EXCEPTION 'Question is not part of this interview session' USING ERRCODE='23503';
    END IF;
    RETURN NEW;
END;
$$;
CREATE TRIGGER interview_answers_question_membership BEFORE INSERT OR UPDATE ON interview_answers
    FOR EACH ROW EXECUTE FUNCTION check_answer_question_membership();
