# 如何连接（VLESS + Vision + REALITY · steal-oneself）

本说明对应仓库内 **`infra/xray-steal-oneself/`** 安装脚本：Xray 监听 **443**，REALITY 回源本机 nginx **`127.0.0.1:8001`**。

安装完成后，在服务器上查看：

- **`/opt/xray-steal-oneself/client-import.json`**
- 或安装结束时终端打印的：**域名（SNI）**、**UUID**、**公钥**、**Short ID**

**SNI** 必须与证书域名一致，且与 Xray `serverNames` 一致。

## Xray 系客户端

1. 将 `client-import.json` 拷到本机或新建配置。
2. 按需把 `vnext[0].address` 改为 VPS 的 **IP 或域名**。
3. 本地 SOCKS `127.0.0.1:10808` / HTTP `127.0.0.1:10809`（与生成文件一致）。

## Clash Meta（Mihomo）

原版 Clash **不支持** VLESS REALITY，请使用 **Clash Meta / Mihomo** 内核客户端。

使用仓库内示例：**`docs/examples/clash-meta-vless-reality.yaml.example`**，替换全部 `REPLACE_*` 后导入。

## 防火墙

- 放行入站 **TCP 443**（若用 HTTP 跳转再放行 **80**）。
- **不要**将 `127.0.0.1:8001` 暴露到公网。

## 自动化 / Agent

字段对照与排错见 **`docs/agent-vless-reality-steal-oneself.md`**。
