# 发版流程（应用内自动更新）

每次发布新版本，按以下步骤操作：

## 1. 改版本号

`src-tauri/tauri.conf.json` 与 `src-tauri/Cargo.toml` 两处同步改为新版本（如 `1.2.0` → `1.3.0`）。

## 2. 带签名打包

```bash
# 用户级环境变量已设置（TAURI_SIGNING_PRIVATE_KEY / PASSWORD），直接：
npm run tauri build
```

打包完成后 `src-tauri/target/release/bundle/nsis/` 下会出现：

- `游戏库_x.y.z_x64-setup.exe`
- `游戏库_x.y.z_x64-setup.exe.sig` ← 更新签名

**签名私钥**：`E:/workbuddy数据/游戏库/.tauri-keys/game-library.key`（已加入 .gitignore，绝不上传；
丢失后所有已装用户将无法收到更新）。

## 3. 生成 latest.json

复制 `release-v1.2.0/` 目录为 `release-v<新版本>/`，修改其中 `latest.json`：

- `version`：新版本号
- `notes`：本次更新说明
- `pub_date`：发布时间的 UTC ISO 字符串
- `platforms.windows-x86_64.signature`：新 `.sig` 文件的**完整内容**
- `platforms.windows-x86_64.url`：指向 Release 资产（安装包建议用 ASCII 名，
  如 `GameLibrary_1.3.0_x64-setup.exe`，避免中文 URL 转义问题）

## 4. 上传到 GitHub Release（已自动化）

Token 已配置（仓库根 `.gh-token`，gitignore 保护）。改版本号、打包、生成
`release-v<版本>/` 后，由 agent 通过 GitHub API 直接完成：创建 Release + 上传
`latest.json` 与 ASCII 名安装包 —— 无需再手动操作浏览器。

（手动备用：浏览器打开 `releases/new?tag=v<版本>`，上传同样两个文件。）

## 5. 更新日志

在 `src/components/ChangelogDialog.tsx` 的 `CHANGELOG` 数组顶部加新版本条目（只写新增功能）。
