#!/bin/sh
# Test certificates for the gateway's TLS tests (and the TLS part of
# crates/harness-core/tests/gateway_vm.rs):
#
#   ca.pem        root of a private CA
#   server.pem    server certificate plus intermediate, like Caddy's
#                 `tls internal` sends it; valid for harness.test,
#                 harness-tls-test, 127.0.0.1 and ::1
#   server.key    its key (PKCS#8)
#   other-ca.pem  an unrelated root, to check that it is not trusted
#
# Valid for 100 years so the tests never expire. The CA keys are thrown
# away. Rerun to replace all files at once: sh generate.sh
set -eu
cd "$(dirname "$0")"
# Git Bash would turn the -subj values into Windows paths (but must keep
# converting the file paths for a native openssl.exe).
export MSYS2_ARG_CONV_EXCL=/CN

days=36500
tmp=$(mktemp -d)
trap 'rm -rf "$tmp"' EXIT

cat > "$tmp/openssl.cnf" <<'EOF'
[req]
distinguished_name = dn
prompt = no
[dn]
CN = unused
[root]
basicConstraints = critical, CA:true
keyUsage = critical, keyCertSign, cRLSign
subjectKeyIdentifier = hash
[intermediate]
basicConstraints = critical, CA:true, pathlen:0
keyUsage = critical, keyCertSign, cRLSign
subjectKeyIdentifier = hash
authorityKeyIdentifier = keyid
[server]
basicConstraints = critical, CA:false
keyUsage = critical, digitalSignature
extendedKeyUsage = serverAuth
subjectAltName = DNS:harness.test, DNS:harness-tls-test, IP:127.0.0.1, IP:::1
subjectKeyIdentifier = hash
authorityKeyIdentifier = keyid
EOF

key() {
    openssl genpkey -algorithm EC -pkeyopt ec_paramgen_curve:P-256 -out "$1"
}

root() { # <name> <subject>
    key "$tmp/$1.key"
    openssl req -x509 -new -key "$tmp/$1.key" -subj "$2" -days "$days" \
        -config "$tmp/openssl.cnf" -extensions root -out "$1.pem"
}

sign() { # <name> <subject> <issuer cert> <issuer key> <extensions>
    key "$tmp/$1.key"
    openssl req -new -key "$tmp/$1.key" -subj "$2" -config "$tmp/openssl.cnf" \
        -out "$tmp/$1.csr"
    openssl x509 -req -in "$tmp/$1.csr" -CA "$3" -CAkey "$4" -days "$days" \
        -set_serial "0x$(openssl rand -hex 8)" \
        -extfile "$tmp/openssl.cnf" -extensions "$5" -out "$tmp/$1.pem"
}

root ca "/CN=harness test root"
root other-ca "/CN=harness test other root"
sign intermediate "/CN=harness test intermediate" ca.pem "$tmp/ca.key" intermediate
sign server "/CN=harness.test" "$tmp/intermediate.pem" "$tmp/intermediate.key" server
cat "$tmp/server.pem" "$tmp/intermediate.pem" > server.pem
cp "$tmp/server.key" server.key
