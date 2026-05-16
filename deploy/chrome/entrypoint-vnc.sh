#!/bin/bash
# Docker Chrome (VNC 可视化模式) 启动脚本
# 1) Xvfb 虚拟显示  2) x11vnc + autocutsel 剪贴板桥  3) noVNC  4) socat CDP  5) Chromium

set -u  # 不用 -e：剪贴板/工具行失败不应阻断主服务

cleanup_locks() {
  rm -f /root/.config/chromium/SingletonLock \
        /root/.config/chromium/SingletonCookie \
        /root/.config/chromium/SingletonSocket \
        /tmp/.X99-lock 2>/dev/null
  rm -f /tmp/.X11-unix/X99 2>/dev/null
}

cleanup_locks

# Xvfb 必须 ready 才能往后走
Xvfb :99 -screen 0 1920x1080x24 -ac +extension RANDR &
XVFB_PID=$!
for i in $(seq 1 30); do
  if xdpyinfo -display :99 >/dev/null 2>&1; then break; fi
  sleep 0.3
done
echo "[entrypoint] Xvfb ready (PID=$XVFB_PID)"

# 剪贴板双向同步（VNC ↔ X11 CLIPBOARD/PRIMARY）
export DISPLAY=:99
autocutsel -fork || echo "[entrypoint] autocutsel CLIPBOARD failed (non-fatal)"
autocutsel -selection PRIMARY -fork || echo "[entrypoint] autocutsel PRIMARY failed (non-fatal)"

# VNC 服务
mkdir -p /root/.vnc
x11vnc -storepasswd "${VNC_PASSWORD:-stagehand}" /root/.vnc/passwd >/dev/null
x11vnc -display :99 \
       -forever -shared \
       -rfbauth /root/.vnc/passwd \
       -rfbport 5900 \
       -bg \
       -o /tmp/x11vnc.log
# -nopw removed: it was overriding -rfbauth and leaving VNC open with no
# password. Anyone reaching :5900 (native VNC) or :6080 (noVNC web) could
# drive the logged-in browser. Now password auth from /root/.vnc/passwd is
# actually enforced. The compose file additionally binds the host ports to
# 127.0.0.1 so the only public path is Cloudflare Tunnel + Access.
echo "[entrypoint] x11vnc on :5900"

# noVNC web 网关
websockify --web=/usr/share/novnc 6080 127.0.0.1:5900 \
  > /tmp/novnc.log 2>&1 &
echo "[entrypoint] noVNC on :6080 (PID=$!)"

# socat：宿主访问 9222 → Chrome 在 127.0.0.1:9223（绕 Chrome DNS-rebinding）
socat TCP-LISTEN:9222,fork,reuseaddr,bind=0.0.0.0 TCP:127.0.0.1:9223 \
  > /tmp/socat.log 2>&1 &
echo "[entrypoint] socat 9222→9223 (PID=$!)"

# 扩展加载：扫描 /opt/extensions/*/manifest.json，拼成 --load-extension=a,b,c
EXT_DIRS=""
if [ -d /opt/extensions ]; then
  for d in /opt/extensions/*/; do
    [ -f "$d/manifest.json" ] && EXT_DIRS="${EXT_DIRS:+$EXT_DIRS,}${d%/}"
  done
fi
EXT_FLAG=""
[ -n "$EXT_DIRS" ] && EXT_FLAG="--load-extension=$EXT_DIRS" && echo "[entrypoint] loading extensions: $EXT_DIRS"

# 主进程：Chromium，会成为 PID 1 (因为 tini 作 init)
echo "[entrypoint] starting chromium ..."
exec chromium \
  --display=:99 \
  --no-first-run \
  --no-default-browser-check \
  --no-sandbox \
  --disable-gpu \
  --disable-dev-shm-usage \
  --remote-debugging-port=9223 \
  --remote-debugging-address=127.0.0.1 \
  --remote-allow-origins=* \
  --user-data-dir=/root/.config/chromium \
  --disable-blink-features=AutomationControlled \
  --use-fake-ui-for-media-stream \
  --use-fake-device-for-media-stream \
  --enable-usermedia-screen-capturing \
  --disable-features=Translate,OptimizationHints,MediaRouter \
  --password-store=basic \
  --lang=en-US \
  $EXT_FLAG \
  --window-position=0,0 \
  --window-size=1920,1080 \
  --start-maximized
