FROM postgres:16-alpine

ENV POSTGRES_DB=matchbox_test
ENV POSTGRES_USER=matchbox
ENV POSTGRES_PASSWORD=matchbox_test

COPY init.sql /docker-entrypoint-initdb.d/01_init.sql
