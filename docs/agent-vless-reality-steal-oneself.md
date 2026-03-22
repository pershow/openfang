# Agent：VLESS + Vision + REALITY（steal-oneself）连接说明

面向自动化或协助用户时使用。安装包路径：**`infra/xray-steal-oneself/install.sh`**；运行时数据：**`/opt/xray-steal-oneself/`**。

## 协议

| 项 | 值 |
|----|-----|
| 协议 | VLESS |
| Flow | `xtls-rprx-vision` |
| 传输 | TCP |
| 安全 | REALITY |
| 服务端口 | 443 |

## 服务端产出字段映射

| 概念 | Xray 客户端 | Clash Meta |
|------|-------------|------------|
| 服务器 | `vnext[0].address` | `proxies[].server` |
| 端口 | 443 | `proxies[].port` |
| UUID | `users[0].id` | `proxies[].uuid` |
| SNI | `realitySettings.serverName` | `proxies[].servername` |
| REALITY 公钥 | `realitySettings.publicKey` | `reality-opts.public-key` |
| Short ID | `realitySettings.shortId` | `reality-opts.short-id` |

**SNI** 须与 nginx 证书域名及服务端 `serverNames` 一致。

## 客户端兼容性

- Xray / sing-box / v2rayN / v2rayNG（新版本）：支持。
- **原版 Clash**：不支持 VLESS REALITY。
- **Clash Meta / Mihomo**：支持，见 `docs/examples/clash-meta-vless-reality.yaml.example`。

## 排错

1. 若 `address` 为域名，DNS 须解析到正确 VPS。
2. 防火墙须放行 TCP 443；nginx 须在 Xray 之前在本机监听 `127.0.0.1:8001`。
3. SNI 与证书不一致会导致 REALITY 握手失败。
4. UUID / 公钥 / Short ID 任一不匹配则认证失败。

## 相关路径

| 路径 | 说明 |
|------|------|
| `/opt/xray-steal-oneself/client-import.json` | 生成的客户端 JSON |
| `/etc/xray/config.json` | 服务端 Xray 配置 |
| `/opt/xray-steal-oneself/nginx-steal-oneself.conf` | nginx REALITY dest |

勿将真实 UUID、密钥、域名提交到公开仓库。
