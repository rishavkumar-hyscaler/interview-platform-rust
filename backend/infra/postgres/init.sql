-- Local development credentials only. Production provisioning uses managed secrets.
CREATE ROLE platform_app
    LOGIN
    PASSWORD 'local_app_password'
    NOSUPERUSER
    NOCREATEDB
    NOCREATEROLE
    NOBYPASSRLS;
