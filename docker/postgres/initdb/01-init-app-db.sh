#!/bin/bash
# Provisions the least-privilege application role and database. The postgres
# entrypoint runs files in /docker-entrypoint-initdb.d ONCE, as the
# superuser, when the data directory is empty (first start).
#
# POSTGRES_USER/POSTGRES_PASSWORD control the bootstrap superuser only; the
# app never authenticates with that identity. APP_PASSWORD (from
# docker-compose.yml, sourced from .env) is used for the role the server
# actually connects as (see DATABASE_URL in docker-compose.yml).
#
# Schema creation is deliberately left to the app's own sqlx migration
# (migrations/0001_init.sql): the "bridge" role owns the "bridge" database,
# so creating a schema in it needs no superuser rights. This script does
# only what does require superuser: creating the role, the database, and
# the role's default search_path.
set -euo pipefail

psql -v ON_ERROR_STOP=1 --username "$POSTGRES_USER" --dbname "$POSTGRES_DB" <<-EOSQL
    CREATE USER bridge PASSWORD '$APP_PASSWORD';
    CREATE DATABASE bridge OWNER bridge;
    ALTER ROLE bridge IN DATABASE bridge SET search_path TO bridge, public;
EOSQL
