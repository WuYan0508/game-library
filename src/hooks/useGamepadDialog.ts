import { useEffect, type RefObject } from "react";
import { setGamepadLayer } from "../services/gamepad";

const FOCUSABLE =
  "button:not(:disabled), input:not(:disabled), select:not(:disabled), [tabindex]:not([tabindex='-1'])";

/**
 * 弹窗/页面的手柄导航（层 2）：
 * - 上/下：在容器内的可交互元素间移动真实焦点（DOM 顺序 ≈ 视觉顺序）
 * - A：激活当前焦点（按钮 / 开关触发 click；文本输入不激活 —— 手柄无键盘）
 * - B：触发 onBack（关闭弹窗 / 返回上级）
 * - 挂载即把本层压到手柄栈顶，卸载自动弹出 —— 多层弹窗天然正确分层
 */
export function useGamepadDialog(
  containerRef: RefObject<HTMLElement | null>,
  opts: { onBack?: () => void; enabled?: boolean } = {},
): void {
  const { onBack, enabled = true } = opts;

  useEffect(() => {
    if (!enabled) {
      return;
    }
    const container = containerRef.current;
    if (!container) {
      return;
    }

    const focusables = () =>
      Array.from(container.querySelectorAll<HTMLElement>(FOCUSABLE)).filter(
        (el) => el.offsetParent !== null,
      );

    const moveFocus = (delta: number) => {
      const list = focusables();
      if (list.length === 0) {
        return;
      }
      const current = list.indexOf(document.activeElement as HTMLElement);
      const index = current >= 0 ? current : 0;
      const target = list[(index + delta + list.length) % list.length];
      target.focus({ preventScroll: true });
    };

    const layerId = `gamepad-dialog-${Math.random().toString(36).slice(2)}`;
    setGamepadLayer(layerId, (action) => {
      if (action === "up") {
        moveFocus(-1);
      } else if (action === "down") {
        moveFocus(1);
      } else if (action === "confirm") {
        const el = document.activeElement as HTMLElement | null;
        // 只激活按钮类元素；文本输入不做激活（手柄无法输入文字）
        if (
          el &&
          (el.tagName === "BUTTON" || el.getAttribute("role") === "switch")
        ) {
          el.click();
        }
      } else if (action === "back") {
        onBack?.();
      }
      // left / right / detail / menu：弹窗内不使用
    });

    // 初始焦点：容器内第一个可交互元素
    const list = focusables();
    if (list.length > 0 && !container.contains(document.activeElement)) {
      list[0].focus({ preventScroll: true });
    }

    return () => {
      setGamepadLayer(layerId, null);
    };
    // onBack 由调用方内联传入；弹窗关闭即卸载本层，无需追求闭包常驻新鲜
  }, [containerRef, enabled, onBack]);
}
