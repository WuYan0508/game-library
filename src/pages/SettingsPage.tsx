import { useEffect, useState } from "react";
import { getVersion } from "@tauri-apps/api/app";
import { disable, enable, isEnabled } from "@tauri-apps/plugin-autostart";
import {
  getStoredAppearance,
  setAppearance,
  type Appearance,
} from "../services/theme";
import { gameApi } from "../services/gameApi";
import { toUserMessage } from "../services/errorMessages";
import { pushToast } from "../services/toastStore";
import styles from "./SettingsPage.module.css";
import ChangelogDialog from "../components/ChangelogDialog";
import UpdateChecker from "../components/UpdateChecker";

interface SettingsPageProps {
  onBack: () => void;
}

const APPEARANCE_OPTIONS: Array<{
  value: Appearance;
  label: string;
  hint: string;
}> = [
  { value: "dark", label: "深色", hint: "默认主题" },
  { value: "light", label: "白色", hint: "明亮主题" },
];

export default function SettingsPage({ onBack }: SettingsPageProps) {
  // null = 还在读取当前状态
  const [autoStart, setAutoStart] = useState<boolean | null>(null);
  const [appearance, setAppearancePref] = useState<Appearance>(
    getStoredAppearance(),
  );
  // null = 还在读取当前状态
  const [closeToTray, setCloseToTray] = useState<boolean | null>(null);
  const [version, setVersion] = useState<string | null>(null);
  const [changelogOpen, setChangelogOpen] = useState(false);
  const [reextracting, setReextracting] = useState(false);
  const [reextractResult, setReextractResult] = useState<string | null>(null);

  useEffect(() => {
    void getVersion()
      .then(setVersion)
      .catch(() => setVersion(null));
  }, []);

  useEffect(() => {
    let cancelled = false;
    void (async () => {
      try {
        const value = await isEnabled();
        if (!cancelled) {
          setAutoStart(value);
        }
      } catch {
        // 读取失败按「未开启」展示；真正切换失败时会有 Toast
        if (!cancelled) {
          setAutoStart(false);
        }
      }
    })();
    return () => {
      cancelled = true;
    };
  }, []);

  useEffect(() => {
    let cancelled = false;
    void (async () => {
      try {
        const value = await gameApi.getCloseToTray();
        if (!cancelled) {
          setCloseToTray(value);
        }
      } catch {
        // 读取失败按「开启」展示（默认行为）
        if (!cancelled) {
          setCloseToTray(true);
        }
      }
    })();
    return () => {
      cancelled = true;
    };
  }, []);

  async function handleReextract() {
    setReextracting(true);
    setReextractResult(null);
    try {
      const report = await gameApi.reextractMissingIcons();
      const parts = [`已修复 ${report.fixed} 个`, `完好 ${report.alreadyOk} 个`];
      if (report.failed.length > 0) {
        parts.push(`失败 ${report.failed.length} 个：${report.failed.map((f) => `${f.name}（${f.reason}）`).join("；")}`);
      }
      setReextractResult(parts.join("，"));
    } catch (error) {
      pushToast(toUserMessage(error));
    } finally {
      setReextracting(false);
    }
  }

  async function toggleCloseToTray() {
    if (closeToTray === null) {
      return;
    }
    const target = !closeToTray;
    setCloseToTray(target);
    try {
      await gameApi.setCloseToTray(target);
    } catch (error) {
      setCloseToTray(!target);
      pushToast(toUserMessage(error));
    }
  }

  async function toggleAutoStart() {
    if (autoStart === null) {
      return;
    }
    const target = !autoStart;
    setAutoStart(target); // 乐观更新，失败回滚
    try {
      if (target) {
        await enable();
      } else {
        await disable();
      }
    } catch (error) {
      setAutoStart(!target);
      pushToast(toUserMessage(error));
    }
  }

  function changeAppearance(pref: Appearance) {
    setAppearancePref(pref);
    setAppearance(pref);
  }

  return (
    <div className={styles.page}>
      <header className={styles.topBar}>
        <button className={styles.backButton} type="button" onClick={onBack}>
          ← 返回游戏库
        </button>
        <h1 className={styles.heading}>设置</h1>
      </header>

      <main className={styles.content}>
        <section className={styles.section}>
          <h2 className={styles.sectionTitle}>开机自启动</h2>
          <div className={styles.sectionBody}>
            <div className={styles.settingRow}>
              <div className={styles.settingText}>
                <span className={styles.settingLabel}>
                  登录 Windows 后自动启动游戏库
                </span>
                <span className={styles.settingDesc}>
                  {autoStart === null
                    ? "正在读取当前状态…"
                    : autoStart
                      ? "已开启：开机后游戏库会自动运行"
                      : "已关闭：需要手动打开游戏库"}
                </span>
              </div>
              <button
                className={
                  autoStart ? `${styles.switch} ${styles.switchOn}` : styles.switch
                }
                type="button"
                role="switch"
                aria-checked={autoStart === true}
                disabled={autoStart === null}
                onClick={() => void toggleAutoStart()}
              >
                <span className={styles.knob} />
              </button>
            </div>
          </div>
        </section>

        <section className={styles.section}>
          <h2 className={styles.sectionTitle}>关闭按钮行为</h2>
          <div className={styles.sectionBody}>
            <div className={styles.settingRow}>
              <div className={styles.settingText}>
                <span className={styles.settingLabel}>
                  关闭时最小化到系统托盘
                </span>
                <span className={styles.settingDesc}>
                  {closeToTray === null
                    ? "正在读取当前状态…"
                    : closeToTray
                      ? "点 ✕ 只是隐藏到托盘；托盘图标右键可选择打开或退出"
                      : "点 ✕ 将直接退出游戏库"}
                </span>
              </div>
              <button
                className={
                  closeToTray
                    ? `${styles.switch} ${styles.switchOn}`
                    : styles.switch
                }
                type="button"
                role="switch"
                aria-checked={closeToTray === true}
                disabled={closeToTray === null}
                onClick={() => void toggleCloseToTray()}
              >
                <span className={styles.knob} />
              </button>
            </div>
          </div>
        </section>

        <section className={styles.section}>
          <h2 className={styles.sectionTitle}>软件外观</h2>
          <div className={styles.sectionBody}>
            <div className={styles.appearanceRow}>
              {APPEARANCE_OPTIONS.map((option) => (
                <button
                  key={option.value}
                  type="button"
                  className={
                    appearance === option.value
                      ? `${styles.appearanceOption} ${styles.selected}`
                      : styles.appearanceOption
                  }
                  onClick={() => changeAppearance(option.value)}
                >
                  <span
                    className={styles.appearancePreview}
                    data-preview={option.value}
                  />
                  <span className={styles.appearanceLabel}>{option.label}</span>
                  <span className={styles.appearanceHint}>{option.hint}</span>
                </button>
              ))}
            </div>
          </div>
        </section>

        <section className={styles.section}>
          <h2 className={styles.sectionTitle}>注意事项</h2>
          <div className={styles.sectionBody}>
            <ol className={styles.notesList}>
              <li>
                该软件用于实现单软件打开电脑内所有游戏，利好多平台玩家以及学习版玩家。
              </li>
              <li>
                当前该软件自动扫描功能仅支持 Steam、Epic 以及 NVIDIA App
                中可识别的软件，后续会开发其他平台的识别。若识别不到想添加的游戏，请手动导入或者指定目录扫描。
              </li>
              <li>
                如需打开 WeGame
                中的游戏，请手动导入游戏启动器（注意是游戏自带的启动器，不是通过
                WeGame 启动的快捷方式）。
              </li>
            </ol>
          </div>
        </section>

        <section className={styles.section}>
          <h2 className={styles.sectionTitle}>维护</h2>
          <div className={styles.sectionBody}>
            <div className={styles.settingRow}>
              <div className={styles.settingText}>
                <span className={styles.settingLabel}>重新提取缺失的图标</span>
                <span className={styles.settingDesc}>
                  扫描库中图标文件丢失的游戏，从启动目标重新提取（用于重装系统 / 清理数据目录后的恢复）
                </span>
              </div>
              <button
                className={styles.actionButton}
                type="button"
                disabled={reextracting}
                onClick={() => void handleReextract()}
              >
                {reextracting ? "提取中…" : "开始修复"}
              </button>
            </div>
            {reextractResult && (
              <p className={styles.maintenanceResult}>{reextractResult}</p>
            )}
          </div>
        </section>

        <section className={styles.section}>
          <h2 className={styles.sectionTitle}>检查更新</h2>
          <div className={styles.sectionBody}>
            <div className={styles.settingRow}>
              <div className={styles.settingText}>
                <span className={styles.settingLabel}>检查并安装更新</span>
                <span className={styles.settingDesc}>
                  需要联网访问 GitHub Releases；仅在你点击时联网，不做后台自动检查
                </span>
              </div>
            </div>
            <UpdateChecker />
          </div>
        </section>

        <section className={styles.section}>
          <h2 className={styles.sectionTitle}>更新日志</h2>
          <div className={styles.sectionBody}>
            <div className={styles.settingRow}>
              <div className={styles.settingText}>
                <span className={styles.settingLabel}>查看各版本新增内容</span>
                <span className={styles.settingDesc}>只记录新增的功能，当前版本 1.2</span>
              </div>
              <button
                className={styles.actionButton}
                type="button"
                onClick={() => setChangelogOpen(true)}
              >
                查看
              </button>
            </div>
          </div>
        </section>

        <p className={styles.versionLine}>
          {version
            ? `游戏库 · 版本 ${version.replace(/\.0$/, "")}`
            : "游戏库"}
        </p>

        {changelogOpen && (
          <ChangelogDialog onClose={() => setChangelogOpen(false)} />
        )}
      </main>
    </div>
  );
}
