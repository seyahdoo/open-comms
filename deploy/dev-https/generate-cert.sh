#!/bin/sh
set -eu

CERT_DIR="${CERT_DIR:-/etc/nginx/dev-https}"
HOSTS_INPUT="${HTTPS_HOSTS:-}"

# Loopback names are always included so local verification works by default.
HOSTS="localhost,127.0.0.1"
if [ -n "$HOSTS_INPUT" ]; then
    HOSTS="$HOSTS,$HOSTS_INPUT"
fi

# Normalize whitespace and duplicates so host changes are compared reliably.
normalized_hosts=""
seen=""
OLD_IFS=$IFS
IFS=,
set -f
for raw in $HOSTS; do
    host=$(printf '%s' "$raw" | sed 's/^[[:space:]]*//; s/[[:space:]]*$//')
    [ -n "$host" ] || continue
    case " $seen " in
        *" $host "*) continue ;;
    esac
    seen="$seen$host "
    if [ -z "$normalized_hosts" ]; then
        normalized_hosts="$host"
    else
        normalized_hosts="$normalized_hosts,$host"
    fi
done
set +f
IFS=$OLD_IFS
HOSTS="$normalized_hosts"

mkdir -p "$CERT_DIR"
HOSTS_FILE="$CERT_DIR/hosts.txt"
WANT_FILE="$CERT_DIR/hosts.want"
printf '%s\n' "$HOSTS" > "$WANT_FILE"

REGENERATE_LEAF=0

if [ ! -s "$CERT_DIR/ca.crt" ] || [ ! -s "$CERT_DIR/ca.key" ]; then
    echo "[open-comms] Generating dev HTTPS CA"
    openssl req -x509 -newkey rsa:2048 -nodes -sha256 -days 3650 \
        -keyout "$CERT_DIR/ca.key" -out "$CERT_DIR/ca.crt" \
        -subj "/CN=Open Comms Dev CA" \
        -addext "basicConstraints=critical,CA:TRUE" \
        -addext "keyUsage=critical,keyCertSign,cRLSign" \
        -addext "subjectKeyIdentifier=hash"
    chmod 600 "$CERT_DIR/ca.key"
    REGENERATE_LEAF=1
fi

if [ "$REGENERATE_LEAF" -eq 0 ] \
    && [ -s "$CERT_DIR/server.crt" ] \
    && [ -s "$CERT_DIR/server.key" ] \
    && [ -s "$HOSTS_FILE" ] \
    && cmp -s "$WANT_FILE" "$HOSTS_FILE"; then
    echo "[open-comms] Reusing dev HTTPS certificate for: $HOSTS"
    rm -f "$WANT_FILE"
    exit 0
fi

echo "[open-comms] Generating dev HTTPS certificate for: $HOSTS"

SAN_FILE="$CERT_DIR/san.cnf"
: > "$SAN_FILE"
cat >> "$SAN_FILE" <<'EOF'
[req]
distinguished_name = dn
req_extensions = v3_req
prompt = no

[dn]
CN = Open Comms Dev

[v3_req]
basicConstraints = CA:FALSE
keyUsage = critical,digitalSignature,keyEncipherment
extendedKeyUsage = serverAuth
subjectAltName = @alt_names

[alt_names]
EOF

index=1
OLD_IFS=$IFS
IFS=,
set -f
for raw in $HOSTS; do
    host=$(printf '%s' "$raw" | sed 's/^[[:space:]]*//; s/[[:space:]]*$//')
    [ -n "$host" ] || continue

    if printf '%s\n' "$host" | grep -Eq '^([0-9]{1,3}\.){3}[0-9]{1,3}$'; then
        printf 'IP.%s = %s\n' "$index" "$host" >> "$SAN_FILE"
    else
        printf 'DNS.%s = %s\n' "$index" "$host" >> "$SAN_FILE"
    fi
    index=$((index + 1))
done
set +f
IFS=$OLD_IFS

openssl req -new -newkey rsa:2048 -nodes -sha256 \
    -keyout "$CERT_DIR/server.key" -out "$CERT_DIR/server.csr" \
    -config "$SAN_FILE"

openssl x509 -req -in "$CERT_DIR/server.csr" \
    -CA "$CERT_DIR/ca.crt" -CAkey "$CERT_DIR/ca.key" -CAcreateserial \
    -out "$CERT_DIR/server.crt" -days 825 -sha256 \
    -extfile "$SAN_FILE" -extensions v3_req

chmod 600 "$CERT_DIR/server.key"
chmod 644 "$CERT_DIR/server.crt" "$CERT_DIR/ca.crt"

rm -f "$CERT_DIR/server.csr"
mv "$WANT_FILE" "$HOSTS_FILE"

echo "[open-comms] Dev HTTPS certificate SANs:"
awk '/^\[alt_names\]/{show=1; next} /^\[/{show=0} show && NF' "$SAN_FILE"
