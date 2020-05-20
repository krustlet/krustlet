#!/usr/bin/env bash
# This file is based upon https://github.com/awslabs/amazon-eks-ami/blob/master/files/bootstrap.sh
# The script is run when a node is started
# Krustlet doesn't yet support TLS bootstraping, so this will generate a server certificate

set -o pipefail
set -o nounset
set -o errexit

function print_help {
    echo "usage: $0 [options] <cluster-name>"
    echo "Bootstraps a Krustlet instance into an EKS cluster"
    echo ""
    echo "-h,--help print this help"
    echo "--krustlet-node-labels Add extra labels to Krustlet."
}

while [[ $# -gt 0 ]]; do
    key="$1"
    case $key in
        -h|--help)
            print_help
            exit 1
            ;;
        --krustlet-node-labels)
            NODE_LABELS=$2
            shift
            shift
            ;;
        *) # unknown option
            print_help
            exit 1
            ;;
    esac
done

NODE_LABELS="${NODE_LABELS:-}"

echo "Generating certificate signing request..."
openssl req -new -sha256 -newkey rsa:2048 -keyout /etc/krustlet/krustlet.key -out /tmp/krustlet.csr -nodes -config <(
cat <<-EOF
[req]
default_bits = 2048
prompt = no
default_md = sha256
req_extensions = req_ext
distinguished_name = dn

[dn]
O=system:nodes
CN=system:node:$(hostname)

[req_ext]
subjectAltName = @alt_names

[alt_names]
IP.1 = $(ip -o -4 addr list eth0 | awk '{print $4}' | cut -d/ -f1)
EOF
)

cat <<EOF > /tmp/csr.yaml
apiVersion: certificates.k8s.io/v1beta1
kind: CertificateSigningRequest
metadata:
  name: $(hostname)
spec:
  request: $(cat /tmp/krustlet.csr | base64 | tr -d '\n')
  usages:
  - digital signature
  - key encipherment
  - server auth
EOF

RETRY_ATTEMPTS=3

for attempt in `seq 0 $RETRY_ATTEMPTS`; do
    rc=0
    
    if [[ $attempt -gt 0 ]]; then
        echo "Retry $attempt of $RETRY_ATTEMPTS to request certificate signing..."
    fi

    echo "Sending certificate signing request..."
    /usr/local/bin/kubectl apply --kubeconfig /etc/eksctl/kubeconfig.yaml -f /tmp/csr.yaml || rc=$?

    if [[ $rc -eq 0 ]]; then
        break
    fi

    if [[ $attempt -eq $RETRY_ATTEMPTS ]]; then
        exit $rc
    fi

    jitter=$((1 + RANDOM % 10))
    sleep_sec="$(( $(( 5 << $((1+$attempt)) )) + $jitter))"
    sleep $sleep_sec
done

for attempt in `seq 0 $RETRY_ATTEMPTS`; do
    rc=0
    
    if [[ $attempt -gt 0 ]]; then
        echo "Retry $attempt of $RETRY_ATTEMPTS to retrieve certificate..."
    fi

    echo "Retrieving certificate from Kubernetes API server..."
    /usr/local/bin/kubectl get --kubeconfig /etc/eksctl/kubeconfig.yaml csr $(hostname) -o jsonpath='{.status.certificate}' > /tmp/krustlet.cert.base64 || rc=$?
    
    if [[ $rc -eq 0 ]] && [ -s /tmp/krustlet.cert.base64 ]; then
        base64 --decode /tmp/krustlet.cert.base64 > /etc/krustlet/krustlet.crt || rc=$?

        if [[ $rc -eq 0 ]]; then
            break
        fi
    fi

    if [[ $attempt -eq $RETRY_ATTEMPTS ]]; then
        exit $rc
    fi

    jitter=$((1 + RANDOM % 10))
    sleep_sec="$(( $(( 5 << $((1+$attempt)) )) + $jitter))"
    sleep $sleep_sec
done

chown root:root /etc/krustlet/krustlet.*
chmod 640 /etc/krustlet/krustlet.*

rm /tmp/krustlet.key /tmp/krustlet.csr

if [[ -n "$NODE_LABELS" ]]; then
    cat <<EOF > /etc/eksctl/krustlet.local.env
NODE_LABELS=$NODE_LABELS
EOF
fi
chown root:root /etc/eksctl/krustlet.local.env

echo "Starting krustlet service..."
systemctl daemon-reload
systemctl enable krustlet
systemctl start krustlet
