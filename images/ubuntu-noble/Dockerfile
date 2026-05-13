FROM ubuntu:noble
ENV container docker
RUN apt-get update && \
    apt-get install -y systemd systemd-sysv openssh-server sudo iproute2 curl wget iputils-ping vim nano net-tools netplan.io lvm2 kmod udev && \
    apt-get clean && rm -rf /var/lib/apt/lists/*
RUN ssh-keygen -A && mkdir -p /run/sshd
RUN echo 'ubuntu:ubuntu' | chpasswd
RUN mv /usr/bin/udevadm /usr/bin/udevadm.bak && \
    echo '#!/bin/bash\nif [[ "$1" == "control" && "$2" == "--reload" ]]; then exit 0; fi\nif [[ "$1" == "trigger" ]]; then exit 0; fi\nexec /usr/bin/udevadm.bak "$@"' > /usr/bin/udevadm && \
    chmod +x /usr/bin/udevadm
RUN systemctl enable ssh systemd-networkd
RUN systemctl mask systemd-udevd.service systemd-udevd-kernel.socket systemd-udevd-control.socket systemd-modules-load.service sys-kernel-debug.mount sys-kernel-tracing.mount
COPY entrypoint.sh /entrypoint.sh
RUN sed -i 's/\r$//' /entrypoint.sh && chmod +x /entrypoint.sh
ENTRYPOINT ["/entrypoint.sh"]
