#!/bin/bash

# Custom Krustlet server install script for Ubuntu 20.04

KRUSTLET_URL=$1
CLUSTER_NAME=$2
RESOURCE_GROUP=$3
SERVICE_IDENTITY_ID=$4
KUBERNETES_VERSION=$5

# update base dependencies
apt update
apt upgrade -y

# install curl
apt install -y curl

# install the Azure CLI
curl -sSL https://aka.ms/InstallAzureCLIDeb | bash

# install kubectl
curl -sSLO "https://storage.googleapis.com/kubernetes-release/release/v$KUBERNETES_VERSION/bin/linux/amd64/kubectl"
chmod 755 kubectl
mv kubectl /usr/local/bin/

# install krustlet
curl -sSL "${KRUSTLET_URL}" | tar -xzf -
mv krustlet-* /usr/local/bin/

# prepare krustlet config directory
mkdir -p /etc/krustlet/config
chown -R krustlet:krustlet /etc/krustlet

# fetch AKS bootstrap credentials
az login --identity -u $SERVICE_IDENTITY_ID
az aks get-credentials -n $CLUSTER_NAME -g $RESOURCE_GROUP
cp /root/.kube/config /etc/krustlet/config/kubeconfig
chown krustlet:krustlet /etc/krustlet/config/kubeconfig

# create a service
cat << EOF > /etc/systemd/system/krustlet.service
[Unit]
Description=Krustlet

[Service]
Restart=on-failure
RestartSec=5s
Environment=KUBECONFIG=/etc/krustlet/config/kubeconfig
Environment=KRUSTLET_CERT_FILE=/etc/krustlet/config/krustlet.crt
Environment=KRUSTLET_PRIVATE_KEY_FILE=/etc/krustlet/config/krustlet.key
Environment=KRUSTLET_DATA_DIR=/etc/krustlet
Environment=RUST_LOG=wasi_provider=info,main=info
Environment=KRUSTLET_BOOTSTRAP_FILE=/etc/krustlet/config/bootstrap.conf
ExecStart=/usr/local/bin/krustlet-wasi
User=krustlet
Group=krustlet

[Install]
WantedBy=multi-user.target
EOF
chmod +x /etc/systemd/system/krustlet.service

systemctl enable krustlet
systemctl start krustlet

sleep 3

kubectl --kubeconfig=/root/.kube/config certificate approve krustlet-wasi-tls
