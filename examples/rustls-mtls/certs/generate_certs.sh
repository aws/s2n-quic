# immediately bail if any command fails
set -e

echo "generating CA private key and certificate"
openssl req -nodes -new -x509 -keyout ca-key.pem -out ca-cert.pem -days 65536 -config config/ca.cnf

# use rsa because it lets us have one fewer config file
# https://www.openssl.org/docs/man1.0.2/man1/openssl-req.html
echo "generating server private key and CSR"
openssl req -sha256 -nodes -newkey rsa:2048 -keyout server-key.pem -out server.csr -config config/server.cnf

echo "generating client private key and CSR"
openssl req -sha256 -nodes -newkey rsa:2048 -keyout client-key.pem -out client.csr -config config/client.cnf

echo "generating server certificate and signing it"
openssl x509 -days 65536 -req -in server.csr -CA ca-cert.pem -CAkey ca-key.pem -CAcreateserial -out server-cert.pem -extensions req_ext -extfile config/server.cnf

echo "generating client certificate and signing it"
openssl x509 -days 65536 -req -in client.csr -CA ca-cert.pem -CAkey ca-key.pem -CAcreateserial -out client-cert.pem -extensions req_ext -extfile config/client.cnf

echo "verifying generated certificates"
openssl verify -CAfile ca-cert.pem server-cert.pem
openssl verify -CAfile ca-cert.pem client-cert.pem

echo "cleaning up temporary files"
rm server.csr
rm client.csr
rm ca-key.pem
