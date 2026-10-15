/** 资源 URL 解析。
 *
 * 数据库里存的是**相对应用数据目录**的路径（`covers/xxx.png`、`icons/xxx.png`）。
 * 这里把它拼成绝对路径，再交给 Tauri 的 asset protocol 转成可放进 `<img src>` 的 URL。
 *
 * 只做字符串拼接，不访问文件系统 —— 文件是否存在由浏览器加载失败（`onError`）来暴露。
 */

import { convertFileSrc } from "@tauri-apps/api/core";
import { gameApi } from "./gameApi";

let dataDirPromise: Promise<string> | null = null;

/** 应用数据根目录，进程内只取一次。 */
function loadDataDir(): Promise<string> {
  dataDirPromise ??= gameApi
    .getAppDataPath()
    .then((paths) => paths.dataDir)
    .catch((error: unknown) => {
      // 失败不缓存，下次可以重试
      dataDirPromise = null;
      throw error;
    });

  return dataDirPromise;
}

/** 把相对路径转成 asset URL；入参为空时返回 null（调用方回退占位图）。 */
export async function resolveAssetUrl(
  relativePath: string | null,
): Promise<string | null> {
  if (relativePath === null || relativePath.trim() === "") {
    return null;
  }

  const base = await loadDataDir();
  // 数据目录的分隔符风格决定拼接方式：Windows 上是 `\`
  const separator = base.includes("\\") ? "\\" : "/";
  const absolute = `${base}${separator}${relativePath.replace(/\//g, separator)}`;

  return convertFileSrc(absolute);
}
