import { invoke } from "@tauri-apps/api/core";
/** 软件外观偏好：深色 / 浅色。
 *
 * 偏好存 localStorage（应用本地 WebView 存储，随应用保留）。
 * 历史遗留值 "system"（跟随系统，已移除的功能）按深色处理。
 */

export type Appearance = "dark" | "light";

/** 通透模式：开启 = Acrylic 透桌面（默认）；关闭 = 窗口不透明背景。 */
const TRANSLUCENT_KEY = "translucent";

const STORAGE_KEY = "appearance";

export function getStoredAppearance(): Appearance {
  return localStorage.getItem(STORAGE_KEY) === "light" ? "light" : "dark";
}

function applyTheme(): void {
  document.documentElement.dataset.theme = getStoredAppearance();
}

export function getStoredTranslucent(): boolean {
  return localStorage.getItem(TRANSLUCENT_KEY) !== "false"; // 默认开启（与历史行为一致）
}

function applyTranslucentAttr(): void {
  document.documentElement.dataset.translucent = getStoredTranslucent() ? "true" : "false";
}

/** 同步窗口通透效果：Rust 端 Acrylic 开关 + 前端透明变量。 */
export async function applyWindowEffects(): Promise<void> {
  applyTranslucentAttr();
  try {
    await invoke("set_translucent", { enabled: getStoredTranslucent() });
  } catch (err) {
    console.error("切换窗口通透效果失败", err);
  }
}

/** 用户切换通透模式（设置页）。 */
export function setTranslucent(enabled: boolean): void {
  localStorage.setItem(TRANSLUCENT_KEY, enabled ? "true" : "false");
  void applyWindowEffects();
}

/** 启动时调用一次：写入初始主题与通透属性。 */
export function initTheme(): void {
  applyTheme();
  applyTranslucentAttr();
}

/** 用户切换外观偏好。 */
export function setAppearance(pref: Appearance): void {
  localStorage.setItem(STORAGE_KEY, pref);
  applyTheme();
}
