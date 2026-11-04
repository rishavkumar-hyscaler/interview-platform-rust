ALTER TABLE candidate_access ADD COLUMN sealed_token text;
ALTER TABLE candidate_access ADD CONSTRAINT candidate_access_session_unique UNIQUE(session_id);
GRANT UPDATE(token_hash,sealed_token) ON candidate_access TO platform_app;
