FROM debian:bookworm-slim

# Install Chromium and dependencies
RUN apt-get update && apt-get install -y \
    chromium \
    curl \
    wget \
    ca-certificates \
    fonts-liberation \
    fonts-noto-cjk \
    fonts-noto-color-emoji \
    libnss3 libgtk-3-0 libdrm2 libgbm1 libasound2 \
    pulseaudio \
    xdg-utils \
    xvfb \
    x11-utils \
    x11-xserver-utils \
    xterm \
    x11vnc \
    novnc \
    websockify \
    autocutsel \
    xclip \
    x11-apps \
    supervisor \
    socat \
    tini \
    --no-install-recommends && \
    rm -rf /var/lib/apt/lists/*

# Create user data directory
RUN mkdir -p /root/.config/chromium && \
    chmod -R 755 /root/.config/chromium && \
    mkdir -p /tmp/vnc

COPY deploy/chrome/entrypoint-vnc.sh /usr/local/bin/entrypoint-vnc.sh
RUN chmod +x /usr/local/bin/entrypoint-vnc.sh

EXPOSE 9222 5900 6080

ENTRYPOINT ["tini", "--", "/usr/local/bin/entrypoint-vnc.sh"]
