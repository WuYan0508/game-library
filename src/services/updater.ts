import { check, type Update } from "@tauri-apps/plugin-updater";

/**
 * 应用内更新：基于 Tauri 官方 updater 插件。
 *
 * 联网时机说明：仅在用户点击「检查更新」或启动后的静默检查时访问
 * GitHub Releases（读取 latest.json），不做任何后台轮询。
 */
export async function checkForUpdate(): Promise<Update | null> {
  return await check();
}

/** 下载并安装更新（Windows NSIS：下载安装包 → 运行安装 → 由调用方决定重启）。 */
export async function downloadAndInstallUpdate(
  update: Update,
  onProgress: (percent: number | null, stage: "downloading" | "installing") => void,
): Promise<void> {
  let total = 0;
  let downloaded = 0;
  await update.downloadAndInstall((event) => {
    switch (event.event) {
      case "Started":
        total = event.data.contentLength ?? 0;
        downloaded = 0;
        onProgress(total > 0 ? 0 : null, "downloading");
        break;
      case "Progress":
        downloaded += event.data.chunkLength ?? 0;
        onProgress(total > 0 ? Math.min(100, Math.round((downloaded / total) * 100)) : null, "downloading");
        break;
      case "Finished":
        onProgress(100, "installing");
        break;
    }
  });
}
