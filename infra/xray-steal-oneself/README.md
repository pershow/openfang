# xray-steal-oneself（可选基建）

基于 [chika0801/Xray-examples · VLESS-Vision-REALITY/steal_oneself](https://github.com/chika0801/Xray-examples/tree/main/VLESS-Vision-REALITY/steal_oneself) 思路：**本机 nginx** 在 `127.0.0.1:8001` 提供与证书一致的 TLS，作为 REALITY 的 `dest`；静态页为占位「登录」样式，无反代外站。

- **安装**：在 Linux 服务器上进入本目录，配置 `DOMAIN` 与证书路径后执行 `install.sh`（见脚本内注释）。
- **连接说明**：仓库根目录 `docs/vless-reality-steal-oneself.md`
- **Agent 说明**：`docs/agent-vless-reality-steal-oneself.md`
- **Clash Meta 示例**：`docs/examples/clash-meta-vless-reality.yaml.example`

与 SiliCrew 控制面（`compile_turn`、scope 等）相互独立；仅在需要自建 VLESS+REALITY 出口时使用。
