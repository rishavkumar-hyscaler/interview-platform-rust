-- Preserve the existing three-set pool for existing jobs; new jobs default to one.
ALTER TABLE jobs ADD COLUMN question_set_count integer NOT NULL DEFAULT 3
    CHECK (question_set_count BETWEEN 1 AND 5);
ALTER TABLE jobs ALTER COLUMN question_set_count SET DEFAULT 1;
