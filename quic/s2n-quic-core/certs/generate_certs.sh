# Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
# SPDX-License-Identifier: Apache-2.0

# immediately bail if any command fails
set -e

echo "generating pem CA private key and certificate"
openssl req -new -newkey ec -pkeyopt ec_paramgen_curve:prime256v1 -x509 -nodes -out cert.pem -keyout key.pem -days 65536 -config config/ca.cnf


echo "generating PKCS #1 encoded CA private key and certificate"
openssl genrsa -f4 -out key_pkcs1.pem 2048
openssl req -new -x509 -key key_pkcs1.pem -out cert_pkcs1.pem -days 65536 -config config/ca.cnf

echo "converting pem to der"
openssl x509 -outform der -inform pem -in cert.pem -out cert.der
openssl pkcs8 -topk8 -nocrypt -outform DER -in key.pem -out key.der

# The following commands can be used to generate new der encoded cert/key
# instead of converting pem to der
# echo "generating der CA private key and certificate"
# openssl genpkey -algorithm EC -pkeyopt ec_paramgen_curve:prime256v1 | \
#     openssl pkcs8 -topk8 -nocrypt -outform DER > key.der
# openssl req -new -x509 -outform DER -keyform DER -key key.der -out cert.der -days 65536 -config config/ca.cnf



# 'untrusted' here means that the cert will be untrusted by other certificates above
echo "generating a cert/key pair to test 'untrusted' behavior"
openssl req -new -newkey rsa:2048 -x509 -nodes -out untrusted_cert.pem -keyout untrusted_key.pem -days 65536 -config config/ca.cnf
