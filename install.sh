#!/bin/sh
sudo curl -s -L -o /usr/local/bin/virtctl \
  https://github.com/omidiyanto/micro-vm-with-docker/releases/latest/download/virtctl-linux-x86_64
sudo chmod a+x /usr/local/bin/virtctl
echo "[DONE] virtctl installed successfully"
/usr/local/bin/virtctl --version