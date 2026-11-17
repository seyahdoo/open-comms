FROM nginx:1.27-alpine

RUN apk add --no-cache openssl

COPY default.conf /etc/nginx/conf.d/default.conf
COPY generate-cert.sh /docker-entrypoint.d/10-open-comms-dev-cert.sh

RUN chmod 0755 /docker-entrypoint.d/10-open-comms-dev-cert.sh
