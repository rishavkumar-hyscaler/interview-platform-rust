-- A session references a prepared set; questions and reference answers are not copied.
ALTER TABLE interview_sessions ADD COLUMN question_set_id uuid;
ALTER TABLE interview_sessions ADD CONSTRAINT interview_sessions_question_set_fk
    FOREIGN KEY (interviewer_id,job_id,question_set_id)
    REFERENCES question_sets(interviewer_id,job_id,id);
UPDATE interview_sessions s SET question_set_id=p.id
FROM question_sets p
WHERE s.generation_id=p.id AND p.status='completed'
    AND s.interviewer_id=p.interviewer_id AND s.job_id=p.job_id;
CREATE INDEX interview_sessions_question_set ON interview_sessions(question_set_id)
    WHERE question_set_id IS NOT NULL;
ALTER TABLE question_sets DROP COLUMN assigned_session_id;

-- One session-scoped read function supports shared sets and standalone questions.
-- SECURITY INVOKER preserves table RLS on PostgreSQL 14 and later.
CREATE FUNCTION session_question_details() RETURNS TABLE (
    id uuid,interviewer_id uuid,session_id uuid,"position" integer,kind text,difficulty text,
    prompt text,rubric jsonb,served_at timestamptz,generation_id uuid,competency_id text,
    job_id uuid,fingerprint text,embedding vector,embedding_model text,
    embedding_dimensions integer,set_id uuid
) LANGUAGE sql STABLE SECURITY INVOKER AS $$
SELECT q.id,q.interviewer_id,s.id AS session_id,q.position,q.kind,q.difficulty,
       q.prompt,q.rubric,q.served_at,q.generation_id,q.competency_id,q.job_id,
       q.fingerprint,q.embedding,q.embedding_model,q.embedding_dimensions,q.set_id
FROM interview_sessions s JOIN interview_questions q
  ON q.interviewer_id=s.interviewer_id AND
     ((s.question_set_id IS NOT NULL AND q.set_id=s.question_set_id)
      OR (s.question_set_id IS NULL AND q.session_id=s.id));
$$;
GRANT EXECUTE ON FUNCTION session_question_details() TO platform_app;

-- Answers belong to a session and a shared question. Retain database-level
-- membership checks so a question from another session/set cannot be answered.
ALTER TABLE interview_answers DROP CONSTRAINT interview_answers_interviewer_id_session_id_question_id_fkey;
-- Older pool assignment moved source questions into the first candidate's session.
-- The set now owns those questions; existing answers retain their session links.
UPDATE interview_questions SET session_id=NULL WHERE set_id IS NOT NULL;
ALTER TABLE interview_questions ADD CONSTRAINT interview_questions_owner_id_key UNIQUE(interviewer_id,id);
ALTER TABLE interview_answers ADD CONSTRAINT interview_answers_question_fk
    FOREIGN KEY (interviewer_id,question_id) REFERENCES interview_questions(interviewer_id,id);
ALTER TABLE interview_answers ADD CONSTRAINT interview_answers_session_fk
    FOREIGN KEY (interviewer_id,session_id) REFERENCES interview_sessions(interviewer_id,id);
CREATE FUNCTION check_answer_question_membership() RETURNS trigger LANGUAGE plpgsql AS $$
BEGIN
    IF NOT EXISTS (SELECT 1 FROM session_question_details()
                   WHERE interviewer_id=NEW.interviewer_id
                     AND session_id=NEW.session_id AND id=NEW.question_id) THEN
        RAISE EXCEPTION 'Question is not part of this interview session' USING ERRCODE='23503';
    END IF;
    RETURN NEW;
END;
$$;
CREATE TRIGGER interview_answers_question_membership
    BEFORE INSERT OR UPDATE ON interview_answers
    FOR EACH ROW EXECUTE FUNCTION check_answer_question_membership();
