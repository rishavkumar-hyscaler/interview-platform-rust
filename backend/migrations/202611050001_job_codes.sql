-- Preserve internal UUID relationships; add the interviewer-facing Job ID.
ALTER TABLE jobs ADD COLUMN job_code text;
UPDATE jobs SET job_code = 'LEGACY-' || upper(replace(id::text, '-', ''));
ALTER TABLE jobs ALTER COLUMN job_code SET NOT NULL;
ALTER TABLE jobs ADD CONSTRAINT jobs_job_code_check
    CHECK (job_code ~ '^[A-Z0-9][A-Z0-9_-]{0,63}$');
ALTER TABLE jobs ADD CONSTRAINT jobs_interviewer_job_code_key
    UNIQUE (interviewer_id, job_code);
