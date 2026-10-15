/** 软件外观偏好：深色 / 浅色。
 *
 * 偏好存 localStorage（应用本地 WebView 存储，随应用保留）。
 * 历史遗留值 "system"（跟随系统，已移除的功能）按深色处理。
 */

export type Appearance = "dark" | "light";

const STORAGE_KEY = "appearance";

export function getStoredAppearance(): Appearance {
  return localStorage.getItem(STORAGE_KEY) === "light" ? "light" : "dark";
}

function applyTheme(): void {
  document.documentElement.dataset.theme = getStoredAppearance();
}

/** 启动时调用一次：写入初始主题。 */
export function initTheme(): void {
  applyTheme();
}

/** 用户切换外观偏好。 */
export function setAppearance(pref: Appearance): void {
  localStorage.setItem(STORAGE_KEY, pref);
  applyTheme();
}
