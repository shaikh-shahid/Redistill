#!/bin/bash
# Generate self-signed TLS certificates for testing
# DO NOT use these certificates in production!

set -e

SCRIPT_DIR="$( cd "$( dirname "${BASH_SOURCE[0]}" )" && pwd )"
CERT_DIR="$SCRIPT_DIR/../certs"

mkdir -p "$CERT_DIR"
cd "$CERT_DIR"

echo "Generating test TLS certificates..."

# Generate private key and certificate in one command
openssl req -x509 -newkey rsa:4096 \
  -keyout server-key.pem \
  -out server-cert.pem \
  -days 365 \
  -nodes \
  -subj "/C=US/ST=Test/L=Test/O=Redistill/OU=Testing/CN=localhost" \
  2>/dev/null

echo ""
echo "✅ Test certificates generated:"
echo "   📄 Certificate: $CERT_DIR/server-cert.pem"
echo "   🔑 Private Key: $CERT_DIR/server-key.pem"
echo ""
echo "⚠️  WARNING: These are self-signed certificates for TESTING ONLY"
echo "    Valid for: 365 days from today"
echo "    Valid for: localhost, 127.0.0.1"
echo ""
echo "For production, use real certificates from:"
echo "  - Let's Encrypt (free)"
echo "  - Your Certificate Authority"
echo "  - Cloud provider (AWS, GCP, Azure)"

