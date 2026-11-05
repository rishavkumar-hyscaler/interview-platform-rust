-- A durable pool of candidate-independent sets; existing session questions stay intact.
ALTER TABLE jobs ADD COLUMN preparation_duration_minutes integer NOT NULL DEFAULT 30
    CHECK (preparation_duration_minutes BETWEEN 5 AND 180);

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
    assigned_session_id uuid UNIQUE,
    created_at timestamptz NOT NULL DEFAULT now(),
    UNIQUE (interviewer_id,job_id,id),
    FOREIGN KEY (interviewer_id,job_id) REFERENCES jobs(interviewer_id,id),
    FOREIGN KEY (interviewer_id,assigned_session_id) REFERENCES interview_sessions(interviewer_id,id)
);
CREATE INDEX question_sets_available ON question_sets(interviewer_id,job_id,duration_minutes,created_at)
    WHERE assigned_session_id IS NULL;
ALTER TABLE question_sets ENABLE ROW LEVEL SECURITY;
ALTER TABLE question_sets FORCE ROW LEVEL SECURITY;
CREATE POLICY interviewer_scope ON question_sets
    USING (interviewer_id = nullif(current_setting('app.interviewer_id',true),'')::uuid)
    WITH CHECK (interviewer_id = nullif(current_setting('app.interviewer_id',true),'')::uuid);
GRANT SELECT,INSERT,UPDATE ON question_sets TO platform_app;

ALTER TABLE interview_questions ALTER COLUMN session_id DROP NOT NULL;
ALTER TABLE interview_questions ADD COLUMN set_id uuid;
ALTER TABLE interview_questions ADD CONSTRAINT questions_set_owner_fk
    FOREIGN KEY (interviewer_id,job_id,set_id) REFERENCES question_sets(interviewer_id,job_id,id);
ALTER TABLE interview_questions ADD CONSTRAINT questions_destination_check
    CHECK (session_id IS NOT NULL OR set_id IS NOT NULL);
ALTER TABLE interview_questions ADD CONSTRAINT questions_set_position_key UNIQUE(set_id,position);
GRANT UPDATE(session_id) ON interview_questions TO platform_app;

-- Existing jobs also receive three queued sets. No sessions or candidates are fabricated.
INSERT INTO question_sets(id,interviewer_id,job_id,duration_minutes,seed)
SELECT gen_random_uuid(),j.interviewer_id,j.id,j.preparation_duration_minutes,gen_random_uuid()
FROM jobs j CROSS JOIN generate_series(1,3);
