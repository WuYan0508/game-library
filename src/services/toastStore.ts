/** 轻量 Toast 状态：模块级 pub-sub，任何组件都能 `pushToast`，无需层层传 props。 */

export type ToastTone = "error" | "info";

export interface ToastItem {
  id: number;
  message: string;
  tone: ToastTone;
}

let sequence = 0;
let items: ToastItem[] = [];
const listeners = new Set<(current: ToastItem[]) => void>();

function emit(): void {
  const snapshot = [...items];
  for (const listener of listeners) {
    listener(snapshot);
  }
}

/** 订阅当前 Toast 列表。返回取消订阅函数；订阅时会立即收到当前快照。 */
export function subscribeToasts(
  listener: (current: ToastItem[]) => void,
): () => void {
  listeners.add(listener);
  listener([...items]);
  return () => {
    listeners.delete(listener);
  };
}

/** 弹一条提示。`timeoutMs` 为 0 表示不自动消失（需手动关闭）。 */
export function pushToast(
  message: string,
  tone: ToastTone = "error",
  timeoutMs = 6000,
): number {
  const id = ++sequence;
  items = [...items, { id, message, tone }];
  emit();

  if (timeoutMs > 0) {
    window.setTimeout(() => dismissToast(id), timeoutMs);
  }
  return id;
}

export function dismissToast(id: number): void {
  if (!items.some((item) => item.id === id)) {
    return;
  }
  items = items.filter((item) => item.id !== id);
  emit();
}
