#!/bin/bash

if [ -n "$VM_IP_CIDR" ] && [ -n "$VM_GW" ]; then
    echo "Setup IP $VM_IP_CIDR..."

    cat <<EOF > /etc/netplan/50-cloud-init.yaml
network:
  version: 2
  renderer: networkd
  ethernets:
    eth0:
      dhcp4: no
      addresses:
        - ${VM_IP_CIDR}
      routes:
        - to: default
          via: ${VM_GW}
      nameservers:
        addresses: [${VM_DNS:-8.8.8.8, 1.1.1.1}]
EOF
    chmod 600 /etc/netplan/50-cloud-init.yaml
fi

exec /lib/systemd/systemd
