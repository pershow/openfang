#!/bin/bash
# VLESS + Vision + REALITY（steal oneself）— nginx TLS dest 127.0.0.1:8001
# 依赖：curl、unzip、openssl、python3、nginx
#
#   export DOMAIN="vpn.example.com"
#   export SSL_FULLCHAIN="/etc/letsencrypt/live/$DOMAIN/fullchain.pem"
#   export SSL_KEY="/etc/letsencrypt/live/$DOMAIN/privkey.pem"
#   sudo -E ./install.sh
#
# 运行时目录固定为 /opt/xray-steal-oneself（与 OpenParlant 控制面文档一致）。
# 本仓库内模板路径：infra/xray-steal-oneself/

set -euo pipefail

[ "$(id -u)" -eq 0 ] || { echo "请使用 root 运行: sudo -E $0"; exit 1; }
[ -n "${DOMAIN:-}" ] || { echo "请设置 DOMAIN=你的域名"; exit 1; }
[ -n "${SSL_FULLCHAIN:-}" ] && [ -f "$SSL_FULLCHAIN" ] || { echo "请设置 SSL_FULLCHAIN 为存在的 fullchain PEM"; exit 1; }
[ -n "${SSL_KEY:-}" ] && [ -f "$SSL_KEY" ] || { echo "请设置 SSL_KEY 为存在的私钥 PEM"; exit 1; }
command -v python3 >/dev/null || { echo "需要 python3"; exit 1; }

SCRIPT_DIR="$(cd "$(dirname "$0")" && pwd)"
INSTALL_ROOT="/opt/xray-steal-oneself"
XRAY_BIN="/usr/local/bin/xray"
CFG="/etc/xray/config.json"

ARCH="$(uname -m)"
case "$ARCH" in
  x86_64) XRAY_ZIP="Xray-linux-64.zip" ;;
  aarch64|arm64) XRAY_ZIP="Xray-linux-arm64-v8a.zip" ;;
  *) echo "不支持的架构: $ARCH"; exit 1 ;;
esac

echo "[1/6] 安装目录 -> $INSTALL_ROOT"
mkdir -p "$INSTALL_ROOT/fake-login" /etc/xray
cp -a "$SCRIPT_DIR/fake-login/." "$INSTALL_ROOT/fake-login/"

echo "[2/6] 下载 Xray ($XRAY_ZIP)"
TMP="$(mktemp -d)"
trap 'rm -rf "$TMP"' EXIT
VER=""
if command -v jq >/dev/null 2>&1; then
  VER="$(curl -fsSL https://api.github.com/repos/XTLS/Xray-core/releases/latest | jq -r .tag_name)"
fi
[ -n "$VER" ] || VER="$(curl -fsSL https://api.github.com/repos/XTLS/Xray-core/releases/latest | sed -n 's/.*"tag_name": *"\([^"]*\)".*/\1/p')"
[ -n "$VER" ] || { echo "无法获取 Xray 版本"; exit 1; }
curl -fsSL -o "$TMP/xray.zip" "https://github.com/XTLS/Xray-core/releases/download/${VER}/${XRAY_ZIP}"
unzip -qo "$TMP/xray.zip" -d "$TMP"
install -m 0755 "$TMP/xray" "$XRAY_BIN"

echo "[3/6] 生成 UUID / REALITY 密钥 / shortId"
UUID="$("$XRAY_BIN" uuid)"
KEY_OUT="$("$XRAY_BIN" x25519)"
PRIV="$(echo "$KEY_OUT" | awk '/Private key:/{print $3}')"
PUB="$(echo "$KEY_OUT" | awk '/Public key:/{print $3}')"
[ -n "$PRIV" ] && [ -n "$PUB" ] || { echo "x25519 解析失败"; exit 1; }
SHORT_ID="$(openssl rand -hex 8)"

echo "[4/6] 写入 $CFG"
python3 - "$SCRIPT_DIR/config_server.json" "$CFG" "$UUID" "$DOMAIN" "$PRIV" "$SHORT_ID" <<'PY'
import json, sys
src, dst, uuid, domain, priv, sid = sys.argv[1:7]
with open(src, encoding="utf-8") as f:
    d = json.load(f)
c = d["inbounds"][0]["settings"]["clients"][0]
c["id"] = uuid
rs = d["inbounds"][0]["streamSettings"]["realitySettings"]
rs["serverNames"] = [domain]
rs["privateKey"] = priv
rs["shortIds"] = [sid]
with open(dst, "w", encoding="utf-8") as f:
    json.dump(d, f, indent=2)
PY

echo "[5/6] 写入 nginx 片段"
python3 - "$SCRIPT_DIR/nginx-steal-oneself.conf" "$INSTALL_ROOT/nginx-steal-oneself.conf" "$DOMAIN" "$SSL_FULLCHAIN" "$SSL_KEY" <<'PY'
import sys
src, dst, domain, chain, key = sys.argv[1:6]
text = open(src, encoding="utf-8").read()
text = text.replace("REPLACE_DOMAIN", domain)
text = text.replace("REPLACE_SSL_FULLCHAIN", chain)
text = text.replace("REPLACE_SSL_KEY", key)
open(dst, "w", encoding="utf-8").write(text)
PY

cp "$SCRIPT_DIR/nginx.conf.example" "$INSTALL_ROOT/nginx.conf.example"

echo "[6/6] 客户端模板 -> $INSTALL_ROOT/client-import.json"
python3 - "$SCRIPT_DIR/config_client.json.example" "$INSTALL_ROOT/client-import.json" "$DOMAIN" "$UUID" "$PUB" "$SHORT_ID" <<'PY'
import json, sys
src, dst, addr, uuid, pub, sid = sys.argv[1:7]
with open(src, encoding="utf-8") as f:
    d = json.load(f)
u = d["outbounds"][0]["settings"]["vnext"][0]["users"][0]
u["id"] = uuid
d["outbounds"][0]["settings"]["vnext"][0]["address"] = addr
rs = d["outbounds"][0]["streamSettings"]["realitySettings"]
rs["serverName"] = addr
rs["publicKey"] = pub
rs["shortId"] = sid
with open(dst, "w", encoding="utf-8") as f:
    json.dump(d, f, indent=2)
PY

cat > /etc/systemd/system/xray.service <<EOF
[Unit]
Description=Xray (VLESS REALITY steal-oneself)
After=network-online.target nginx.service
Wants=network-online.target

[Service]
Type=simple
ExecStart=$XRAY_BIN run -config $CFG
Restart=on-failure
RestartSec=3
LimitNOFILE=1048576

[Install]
WantedBy=multi-user.target
EOF

systemctl daemon-reload
systemctl enable xray.service

echo ""
echo "=== 完成 ==="
echo "域名 (SNI): $DOMAIN"
echo "UUID:       $UUID"
echo "公钥:       $PUB"
echo "Short ID:   $SHORT_ID"
echo ""
echo "后续步骤:"
echo "  1) 在 nginx 的 http {} 内 include $INSTALL_ROOT/nginx-steal-oneself.conf"
echo "     （完整示例见 $INSTALL_ROOT/nginx.conf.example）"
echo "  2) nginx -t && systemctl reload nginx"
echo "  3) systemctl start xray && systemctl status xray --no-pager"
echo "  4) 客户端: $INSTALL_ROOT/client-import.json"
echo "  5) 说明文档（本仓库）: docs/vless-reality-steal-oneself.md、docs/agent-vless-reality-steal-oneself.md"
echo "     Clash Meta 示例: docs/examples/clash-meta-vless-reality.yaml.example"
echo ""
