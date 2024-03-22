# Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
# SPDX-License-Identifier: Apache-2.0

# immediately bail if any command fails
set -e

# generate PKCS#1 encoded RSA key (openSSL 1.1.1)
echo "generating PKCS #1 encoded RSA key"
openssl genrsa -f4 -out key_pkcs1.pem 2048
