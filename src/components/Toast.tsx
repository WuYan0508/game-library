/** 全局 Toast 容器。挂在 App 根节点，内容由 `toastStore` 驱动。 */

import { useEffect, useState } from "react";
import {
  dismissToast,
  subscribeToasts,
  type ToastItem,
} from "../services/toastStore";
import styles from "./Toast.module.css";

export default function ToastHost() {
  const [items, setItems] = useState<ToastItem[]>([]);

  useEffect(() => subscribeToasts(setItems), []);

  return (
    <div className={styles.host} aria-live="polite">
      {items.map((item) => (
        <div
          key={item.id}
          className={item.tone === "error" ? styles.error : styles.info}
          role={item.tone === "error" ? "alert" : "status"}
        >
          <span className={styles.message}>{item.message}</span>
          <button
            className={styles.close}
            type="button"
            aria-label="关闭提示"
            onClick={() => dismissToast(item.id)}
          >
            ✕
          </button>
        </div>
      ))}
    </div>
  );
}
