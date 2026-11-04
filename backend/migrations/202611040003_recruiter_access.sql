-- Preserve existing interview data and credentials while removing product-level
-- owner/workspace roles. Internal organization IDs remain isolation boundaries.
ALTER TABLE users ADD COLUMN interview_scope_id uuid REFERENCES organizations(id) DEFERRABLE INITIALLY DEFERRED;
UPDATE users u SET interview_scope_id=(
    SELECT m.organization_id FROM memberships m JOIN organizations o ON o.id=m.organization_id
    WHERE m.user_id=u.id ORDER BY o.created_at,m.organization_id LIMIT 1
);
SET CONSTRAINTS ALL IMMEDIATE;
ALTER TABLE users ALTER COLUMN interview_scope_id SET NOT NULL;
ALTER TABLE memberships DROP CONSTRAINT memberships_role_check;
UPDATE memberships SET role='recruiter';
ALTER TABLE memberships ADD CONSTRAINT memberships_role_check CHECK(role='recruiter');
