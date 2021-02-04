#!/usr/bin/env bash

set -o pipefail
set -o nounset
set -o errexit
IFS=$'\n\t'

TEMPLATE_DIR=${TEMPLATE_DIR:-/tmp/worker}

################################################################################
### Validate Required Arguments ################################################
################################################################################
validate_env_set() {
    (
        set +o nounset

        if [ -z "${!1}" ]; then
            echo "Packer variable '$1' was not set. Aborting"
            exit 1
        fi
    )
}

validate_env_set KRUSTLET_VERSION
validate_env_set KRUSTLET_SRC

################################################################################
### Machine Architecture #######################################################
################################################################################

MACHINE=$(uname -m)
if [ "$MACHINE" == "x86_64" ]; then
    ARCH="amd64"
else
    echo "Unknown machine architecture '$MACHINE'" >&2
    exit 1
fi

################################################################################
### Packages ###################################################################
################################################################################

# Update the OS to begin with to catch up to the latest packages.
sudo yum update -y

# Install necessary packages
sudo yum install -y \
    aws-cfn-bootstrap \
    awscli \
    chrony \
    conntrack \
    curl \
    jq \
    ec2-instance-connect \
    nfs-utils \
    socat \
    unzip \
    wget \
    git \
    openssl-devel

sudo yum group install -y "Development Tools"

# Remove the ec2-net-utils package, if it's installed. This package interferes with the route setup on the instance.
if yum list installed | grep ec2-net-utils; then sudo yum remove ec2-net-utils -y -q; fi

################################################################################
### Time #######################################################################
################################################################################

# Make sure Amazon Time Sync Service starts on boot.
sudo chkconfig chronyd on

# Make sure that chronyd syncs RTC clock to the kernel.
cat <<EOF | sudo tee -a /etc/chrony.conf
# This directive enables kernel synchronisation (every 11 minutes) of the
# real-time clock. Note that it can’t be used along with the 'rtcfile' directive.
rtcsync
EOF

# If current clocksource is xen, switch to tsc
if grep --quiet xen /sys/devices/system/clocksource/clocksource0/current_clocksource &&
  grep --quiet tsc /sys/devices/system/clocksource/clocksource0/available_clocksource; then
    echo "tsc" | sudo tee /sys/devices/system/clocksource/clocksource0/current_clocksource
else
    echo "tsc as a clock source is not applicable, skipping."
fi

################################################################################
### kubectl ####################################################################
################################################################################

curl -o kubectl https://amazon-eks.s3.us-west-2.amazonaws.com/1.15.10/2020-02-22/bin/linux/amd64/kubectl
sudo chmod +x kubectl
sudo chown root:root kubectl
sudo mv kubectl /usr/local/bin 

################################################################################
### Krustlet ###################################################################
################################################################################

# Install a Rust toolchain
curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh -s -- -y
export PATH=$PATH:$HOME/.cargo/bin

# Build krustlet to link against the system libssl
# Amazon Linux has an older openssl version than the krustlet release binary
# TODO: make the krustlet to build (wasi or wasmcloud) configurable
echo "Downloading Krustlet source from $KRUSTLET_SRC"
curl $KRUSTLET_SRC -L -o /tmp/krustlet.tar.gz

echo "Unzipping Krustlet source"
mkdir /tmp/krustlet
tar xvzf /tmp/krustlet.tar.gz --strip=1 -C /tmp/krustlet

cargo build --release --manifest-path /tmp/krustlet/Cargo.toml --bin krustlet-wasi
sudo mv /tmp/krustlet/target/release/krustlet-wasi /usr/local/bin/krustlet
rm -rf /tmp/krustlet /tmp/krustlet.tar.gz
sudo chown root:root /usr/local/bin/krustlet
sudo chmod 755 /usr/local/bin/krustlet

sudo mkdir /etc/krustlet

sudo mkdir -p /etc/systemd/system/krustlet.service.d
sudo mv $TEMPLATE_DIR/krustlet.service /etc/systemd/system/krustlet.service
sudo chown root:root /etc/systemd/system/krustlet.service

sudo systemctl daemon-reload
sudo systemctl disable krustlet

################################################################################
### IAM Authenticator ##########################################################
################################################################################

# This is currently needed for the eksctl kubeconfig
curl -o aws-iam-authenticator https://amazon-eks.s3.us-west-2.amazonaws.com/1.15.10/2020-02-22/bin/linux/amd64/aws-iam-authenticator
chmod +x aws-iam-authenticator
sudo chown root:root aws-iam-authenticator
sudo mv aws-iam-authenticator /usr/bin/aws-iam-authenticator

################################################################################
### EKS ########################################################################
################################################################################

sudo mkdir -p /etc/eks
sudo mv $TEMPLATE_DIR/bootstrap.sh /etc/eks/bootstrap.sh
sudo chmod +x /etc/eks/bootstrap.sh

################################################################################
### AMI Metadata ###############################################################
################################################################################

BASE_AMI_ID=$(curl -s  http://169.254.169.254/latest/meta-data/ami-id)
cat <<EOF > /tmp/release
BASE_AMI_ID="$BASE_AMI_ID"
BUILD_TIME="$(date)"
BUILD_KERNEL="$(uname -r)"
ARCH="$(uname -m)"
EOF
sudo mv /tmp/release /etc/eks/release
sudo chown -R root:root /etc/eks

################################################################################
### Cleanup ####################################################################
################################################################################

# Clean up the Rust toolchain
rustup self uninstall -y

# Clean up yum caches to reduce the image size
sudo yum autoremove -y git openssl-devel
sudo yum group remove -y "Development Tools"
sudo yum clean all
sudo rm -rf \
    $TEMPLATE_DIR  \
    /var/cache/yum

# Clean up files to reduce confusion during debug
sudo rm -rf \
    /etc/hostname \
    /etc/machine-id \
    /etc/resolv.conf \
    /etc/ssh/ssh_host* \
    /home/ec2-user/.ssh/authorized_keys \
    /root/.ssh/authorized_keys \
    /var/lib/cloud/data \
    /var/lib/cloud/instance \
    /var/lib/cloud/instances \
    /var/lib/cloud/sem \
    /var/lib/dhclient/* \
    /var/lib/dhcp/dhclient.* \
    /var/lib/yum/history \
    /var/log/cloud-init-output.log \
    /var/log/cloud-init.log \
    /var/log/secure \
    /var/log/wtmp

sudo touch /etc/machine-id
