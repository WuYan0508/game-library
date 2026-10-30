import { useEffect, useState } from "react";
import { getGamepadState, onGamepadState } from "../services/gamepad";
import styles from "./GamepadHintBar.module.css";

/**
 * 手柄操作提示条（界面底部）：
 * 仅在「手柄控制已开启 且 检测到手柄连接」时出现；
 * 内容跟随手柄层的栈顶层自动切换（游戏库 / 各弹窗 / 设置页）。
 * 手柄拔出或未激活时自动隐藏 —— 键鼠用户完全看不到它。
 */
export default function GamepadHintBar() {
  const [state, setState] = useState(getGamepadState());

  useEffect(() => onGamepadState(() => setState(getGamepadState())), []);

  if (
    !state.enabled ||
    !state.connected ||
    !state.active ||
    state.hints === null ||
    state.hints.length === 0
  ) {
    return null;
  }

  return (
    <div className={styles.bar} aria-hidden="true">
      {state.hints.map((hint) => (
        <span key={hint.key + hint.label} className={styles.item}>
          <span className={styles.padBtn}>{hint.key}</span>
          <span className={styles.label}>{hint.label}</span>
        </span>
      ))}
    </div>
  );
}
