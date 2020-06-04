#!/bin/bash

set -euo pipefail

token_id=$(< /dev/urandom tr -dc a-z0-9 | head -c${1:-6};echo;)
# This needs to use /dev/random so it is cryptographically safe
token_secret=$(< /dev/random tr -dc a-z0-9 | head -c${1:-16};echo;)

cat <<EOF | kubectl apply -f -
apiVersion: v1
kind: Secret
metadata:
  name: bootstrap-token-$(echo -n $token_id)
  namespace: kube-system
type: bootstrap.kubernetes.io/token
stringData:
  auth-extra-groups: system:bootstrappers:kubeadm:default-node-token
  expiration: $(date -u +"%Y-%m-%dT%H:%M:%SZ" --date "1 hour")
  token-id: $(echo -n $token_id)
  token-secret: $(echo -n $token_secret)
  usage-bootstrap-authentication: "true"
  usage-bootstrap-signing: "true"
EOF

# Helpful script taken from the armory docs: https://docs.armory.io/spinnaker-install-admin-guides/manual-service-account/
# and modified to suit our needs

CONTEXT=$(kubectl config current-context)
NAMESPACE=kube-system

NEW_CONTEXT=tls-bootstrap-token-user@kubernetes
config_dir=${CONFIG_DIR:-$HOME/.krustlet/config}
file_name=${FILE_NAME:-bootstrap.conf}
KUBECONFIG_FILE="${config_dir}/${file_name}"
TOKEN_USER=tls-bootstrap-token-user

mkdir -p ${config_dir}

TOKEN=$(echo -n "${token_id}.${token_secret}")

# Create dedicated kubeconfig
# Create a full copy
kubectl config view --raw > ${KUBECONFIG_FILE}.full.tmp
# Switch working context to correct context
kubectl --kubeconfig ${KUBECONFIG_FILE}.full.tmp config use-context ${CONTEXT}
# Minify
kubectl --kubeconfig ${KUBECONFIG_FILE}.full.tmp \
  config view --flatten --minify > ${KUBECONFIG_FILE}.tmp
# Rename context
kubectl config --kubeconfig ${KUBECONFIG_FILE}.tmp \
  rename-context ${CONTEXT} ${NEW_CONTEXT}
# Create token user
kubectl config --kubeconfig ${KUBECONFIG_FILE}.tmp \
  set-credentials ${TOKEN_USER} \
  --token ${TOKEN}
# Set context to use token user
kubectl config --kubeconfig ${KUBECONFIG_FILE}.tmp \
  set-context ${NEW_CONTEXT} --user ${TOKEN_USER}
# Set context to correct namespace
kubectl config --kubeconfig ${KUBECONFIG_FILE}.tmp \
  set-context ${NEW_CONTEXT} --namespace ${NAMESPACE}
# Flatten/minify kubeconfig
kubectl config --kubeconfig ${KUBECONFIG_FILE}.tmp \
  view --flatten --minify > ${KUBECONFIG_FILE}
# Remove tmp
rm ${KUBECONFIG_FILE}.full.tmp
rm ${KUBECONFIG_FILE}.tmp
