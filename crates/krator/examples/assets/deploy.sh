#!/bin/bash

# Create CA Key
openssl genrsa -out ca.key 2048

# Create CA Cert
openssl req -new -key ca.key -x509 -out ca.crt -days 3650 -subj "/CN=ca"

# Create Server Key and Signing Request
openssl req -new -nodes -newkey rsa:2048 -keyout server.key -out server.req -batch -subj "/CN=moose-admission-webhook.default.svc" 

# Create Signed Server Cert
openssl x509 -req -in server.req -CA ca.crt -CAkey ca.key -CAcreateserial -out server.crt -days 3650 -sha256

case `uname` in
        Darwin)
            b64_opts='-b=0'
            export ENDPOINT=$(ipconfig getifaddr en0)
            ;;
        *)
            b64_opts='--wrap=0'
            export ENDPOINT="172.17.0.1"
esac

export CA_BUNDLE=$(cat ca.crt | base64 ${b64_opts})

# Configure Admission Webhook
cat webhook.yaml | envsubst | kubectl apply -f -

rm server.req
rm ca.srl
rm ca.key
rm ca.crt
