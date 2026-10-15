/** 自定义标题栏：拖拽、窗口控制按钮、缩放热区。
 *
 * `decorations: false` 之后系统标题栏没了，这里补齐三件事：
 * 1. 拖拽移动 —— `data-tauri-drag-region`（双击空白区最大化由 Tauri 内建）；
 * 2. 最小化 / 最大化 / 关闭按钮；
 * 3. 八个方向缩放热区 —— 无边框窗口默认不能拖边缩放，用 `startResizeDragging` 实现。
 */

import { useEffect, useState } from "react";
import { getCurrentWindow } from "@tauri-apps/api/window";
import styles from "./TitleBar.module.css";

/** 缩放方向。当前 @tauri-apps/api 版本未导出该类型，按 window.d.ts 的字面量本地声明。 */
type ResizeDirection =
  | "East"
  | "North"
  | "NorthEast"
  | "NorthWest"
  | "South"
  | "SouthEast"
  | "SouthWest"
  | "West";

/** 缩放热区：方向 → 样式类。 */
const HANDLES: Array<[ResizeDirection, string]> = [
  ["North", styles.handleN],
  ["South", styles.handleS],
  ["East", styles.handleE],
  ["West", styles.handleW],
  ["NorthEast", styles.handleNE],
  ["NorthWest", styles.handleNW],
  ["SouthEast", styles.handleSE],
  ["SouthWest", styles.handleSW],
];

export default function TitleBar() {
  const appWindow = getCurrentWindow();
  const [maximized, setMaximized] = useState(false);

  useEffect(() => {
    let disposed = false;
    let unlisten: (() => void) | undefined;

    void (async () => {
      // 窗口尺寸变化时同步最大化状态（还原后徽标要变回「最大化」）
      const stop = await appWindow.onResized(async () => {
        setMaximized(await appWindow.isMaximized());
      });
      if (disposed) {
        stop();
      } else {
        unlisten = stop;
      }
    })();

    void appWindow.isMaximized().then((value) => {
      if (!disposed) {
        setMaximized(value);
      }
    });

    return () => {
      disposed = true;
      unlisten?.();
    };
  }, [appWindow]);

  return (
    <>
      <header className={styles.bar} data-tauri-drag-region>
        <span className={styles.title} data-tauri-drag-region>
          游戏库
        </span>

        <div className={styles.controls}>
          <button
            className={styles.control}
            type="button"
            title="最小化"
            onClick={() => void appWindow.minimize()}
          >
            <svg viewBox="0 0 10 10" aria-hidden="true">
              <line x1="1" y1="5" x2="9" y2="5" />
            </svg>
          </button>
          <button
            className={styles.control}
            type="button"
            title={maximized ? "还原" : "最大化"}
            onClick={() => void appWindow.toggleMaximize()}
          >
            {maximized ? (
              <svg viewBox="0 0 10 10" aria-hidden="true">
                <rect x="0.5" y="2.5" width="6" height="6" />
                <polyline points="2.5,2.5 2.5,0.5 9,0.5 9,7 7,7" />
              </svg>
            ) : (
              <svg viewBox="0 0 10 10" aria-hidden="true">
                <rect x="1" y="1" width="8" height="8" />
              </svg>
            )}
          </button>
          <button
            className={`${styles.control} ${styles.closeControl}`}
            type="button"
            title="关闭"
            onClick={() => void appWindow.close()}
          >
            <svg viewBox="0 0 10 10" aria-hidden="true">
              <line x1="1" y1="1" x2="9" y2="9" />
              <line x1="9" y1="1" x2="1" y2="9" />
            </svg>
          </button>
        </div>
      </header>

      {/* 缩放热区：无边框窗口的系统缩放没了，自己补 */}
      {HANDLES.map(([direction, className]) => (
        <div
          key={direction}
          className={className}
          onMouseDown={(event) => {
            event.preventDefault();
            void appWindow.startResizeDragging(direction);
          }}
        />
      ))}
    </>
  );
}
