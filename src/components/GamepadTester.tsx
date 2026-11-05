import { useEffect, useRef, useState } from "react";
import styles from "./GamepadTester.module.css";

interface PadSnapshot {
  id: string;
  mapping: string;
  buttons: boolean[];
  axes: number[];
}

/** 手柄实时测试面板：显示每个已连接手柄的按键与轴状态，供任意类型手柄自查识别情况。 */
export default function GamepadTester() {
  const [snapshot, setSnapshot] = useState<PadSnapshot[]>([]);
  const rafRef = useRef<number | null>(null);

  useEffect(() => {
    const tick = () => {
      rafRef.current = requestAnimationFrame(tick);
      const pads = Array.from(navigator.getGamepads?.() ?? []).filter(
        (pad): pad is Gamepad => pad !== null,
      );
      setSnapshot(
        pads.map((pad) => ({
          id: pad.id,
          mapping: pad.mapping || "未知布局",
          buttons: pad.buttons.map((b) => b.pressed),
          axes: pad.axes.map((a) => Math.round(a * 100) / 100),
        })),
      );
    };
    rafRef.current = requestAnimationFrame(tick);
    return () => {
      if (rafRef.current !== null) {
        cancelAnimationFrame(rafRef.current);
      }
    };
  }, []);

  if (snapshot.length === 0) {
    return (
      <p className={styles.empty}>
        未检测到手柄 —— 连接后按一下任意键，这里会实时显示所有按键与摇杆状态
      </p>
    );
  }

  return (
    <div className={styles.wrap}>
      {snapshot.map((pad) => (
        <div key={pad.id} className={styles.padCard}>
          <div className={styles.padHead}>
            <span className={styles.padName} title={pad.id}>
              {pad.id}
            </span>
            <span className={styles.padMapping}>
              {pad.mapping === "standard" ? "标准布局" : pad.mapping}
            </span>
          </div>
          <div className={styles.padBody}>
            <div className={styles.section}>
              <span className={styles.sectionLabel}>
                按键（{pad.buttons.length} 个）
              </span>
              <div className={styles.buttonGrid}>
                {pad.buttons.map((pressed, index) => (
                  <span
                    key={index}
                    className={`${styles.buttonChip} ${pressed ? styles.buttonOn : ""}`}
                  >
                    {index}
                  </span>
                ))}
              </div>
            </div>
            <div className={styles.section}>
              <span className={styles.sectionLabel}>轴</span>
              <div className={styles.axisList}>
                {pad.axes.map((value, index) => (
                  <div key={index} className={styles.axisRow}>
                    <span className={styles.axisName}>轴 {index}</span>
                    <span className={styles.axisTrack}>
                      <span
                        className={styles.axisDot}
                        style={{ left: `${((value + 1) / 2) * 100}%` }}
                      />
                    </span>
                    <span className={styles.axisValue}>{value.toFixed(2)}</span>
                  </div>
                ))}
              </div>
            </div>
          </div>
        </div>
      ))}
    </div>
  );
}
