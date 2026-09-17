#!/usr/bin/env bash
# Regenerates every PEM in this directory: a throwaway CA, a server certificate for the
# names the bridge tests dial, a server certificate that carries only the production name,
# a client identity for the mTLS case, and a second CA nothing here is signed by. Run it
# from anywhere; it writes next to itself. Ten years of validity, so the suite does not
# start failing on a calendar date — regenerate before 2036 or when a key type stops being
# accepted by rustls. CA keys are not kept: nothing needs to sign against them later.
set -euo pipefail
cd "$(dirname "$0")"
tmp=$(mktemp -d)
trap 'rm -rf "$tmp"' EXIT

days=3650
key() { openssl genpkey -algorithm EC -pkeyopt ec_paramgen_curve:P-256 -out "$1" 2>/dev/null; }

key "$tmp/ca.key"
openssl req -x509 -new -key "$tmp/ca.key" -days "$days" -subj "/CN=EV bridge test CA" \
  -addext "basicConstraints=critical,CA:TRUE" -addext "keyUsage=critical,keyCertSign,cRLSign" \
  -out ca.pem

# Signs a leaf against the test CA: $1 = basename, $2 = subject CN, $3 = SAN list, $4 = EKU.
leaf() {
  key "$1.key"
  openssl req -new -key "$1.key" -subj "/CN=$2" -out "$tmp/$1.csr"
  openssl x509 -req -in "$tmp/$1.csr" -CA ca.pem -CAkey "$tmp/ca.key" -CAcreateserial -days "$days" \
    -extfile <(printf 'subjectAltName=%s\nextendedKeyUsage=%s\nbasicConstraints=CA:FALSE\n' "$3" "$4") \
    -out "$1.pem" 2>/dev/null
}

leaf server concierge "DNS:localhost,DNS:concierge,IP:127.0.0.1" serverAuth
leaf server-concierge-only concierge "DNS:concierge" serverAuth
leaf client piggybank "DNS:piggybank" clientAuth

key "$tmp/other-ca.key"
openssl req -x509 -new -key "$tmp/other-ca.key" -days "$days" -subj "/CN=Some other CA" \
  -addext "basicConstraints=critical,CA:TRUE" -addext "keyUsage=critical,keyCertSign,cRLSign" \
  -out other-ca.pem

rm -f ca.srl
