import { useEffect, useRef, useState } from "react";
import { open } from "@tauri-apps/plugin-dialog";
import { describeError, gameApi } from "../services/gameApi";
import type { ExeProbeResult, ShortcutProbeResult } from "../types/importer";
import { formatBytes } from "../utils/format";
import styles from "./AddGameDialog.module.css";

interface AddGameDialogProps {
  onClose: () => void;
  /** 导入成功后回调，用于刷新列表。 */
  onImported: () => void;
}

type Step = "choose" | "probing" | "ready";

type Probe =
  | { kind: "exe"; data: ExeProbeResult }
  | { kind: "shortcut"; data: ShortcutProbeResult };

const EXE_FILTERS = [{ name: "可执行文件", extensions: ["exe"] }];
const LNK_FILTERS = [{ name: "快捷方式", extensions: ["lnk"] }];
const IMAGE_FILTERS = [
  { name: "图片", extensions: ["png", "jpg", "jpeg", "webp"] },
];

export default function AddGameDialog({
  onClose,
  onImported,
}: AddGameDialogProps) {
  const [step, setStep] = useState<Step>("choose");
  const [probe, setProbe] = useState<Probe | null>(null);
  const [name, setName] = useState("");
  /** 快捷方式目标缺失时，用户手动选定的启动目标。 */
  const [overridePath, setOverridePath] = useState<string | null>(null);
  /** 用户选择的自定义封面（尚未导入，只是本地路径）。 */
  const [coverSourcePath, setCoverSourcePath] = useState<string | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [saving, setSaving] = useState(false);

  const nameInputRef = useRef<HTMLInputElement>(null);

  // 派生量需要在键盘事件里用到，所以放在 effect 之前声明
  const duplicate = probe?.data.duplicate ?? null;
  const targetMissing = probe?.kind === "shortcut" && !probe.data.targetExists;
  const needsOverride = targetMissing && overridePath === null;
  const blocked = duplicate !== null || needsOverride;
  const canSubmit = step === "ready" && !saving && !blocked && name.trim().length > 0;

  useEffect(() => {
    const onKeyDown = (event: KeyboardEvent) => {
      if (event.key === "Escape" && !saving) {
        onClose();
        return;
      }
      // Enter = 确认导入（预览态且可提交时）
      if (event.key === "Enter" && canSubmit) {
        event.preventDefault();
        void confirmImport();
      }
    };
    window.addEventListener("keydown", onKeyDown);
    return () => window.removeEventListener("keydown", onKeyDown);
  });

  useEffect(() => {
    if (step === "ready") {
      nameInputRef.current?.focus();
      nameInputRef.current?.select();
    }
  }, [step]);

  function resetToChoose() {
    setProbe(null);
    setOverridePath(null);
    setCoverSourcePath(null);
    setError(null);
    setStep("choose");
  }

  async function chooseExe() {
    setError(null);
    try {
      const selected = await open({
        multiple: false,
        directory: false,
        filters: EXE_FILTERS,
      });
      if (typeof selected !== "string") {
        return;
      }

      setStep("probing");
      const result = await gameApi.probeExe(selected);
      setProbe({ kind: "exe", data: result });
      setName(result.defaultName);
      setOverridePath(null);
      setStep("ready");
    } catch (err) {
      setProbe(null);
      setStep("choose");
      setError(describeError(err));
    }
  }

  async function chooseShortcut() {
    setError(null);
    try {
      const selected = await open({
        multiple: false,
        directory: false,
        filters: LNK_FILTERS,
      });
      if (typeof selected !== "string") {
        return;
      }

      setStep("probing");
      const result = await gameApi.probeShortcut(selected);
      setProbe({ kind: "shortcut", data: result });
      setName(result.defaultName);
      setOverridePath(null);
      setStep("ready");
    } catch (err) {
      setProbe(null);
      setStep("choose");
      setError(describeError(err));
    }
  }

  /** 目标缺失时手动指定启动目标。走 probeExe 复用校验与规范化。 */
  async function chooseOverrideTarget() {
    setError(null);
    try {
      const selected = await open({
        multiple: false,
        directory: false,
        filters: EXE_FILTERS,
      });
      if (typeof selected !== "string") {
        return;
      }
      const result = await gameApi.probeExe(selected);
      setOverridePath(result.launchPath);
    } catch (err) {
      setError(describeError(err));
    }
  }

  /** 选封面。这里只记路径，真正的校验与复制由后端在导入时完成。 */
  async function chooseCover() {
    setError(null);
    try {
      const selected = await open({
        multiple: false,
        directory: false,
        filters: IMAGE_FILTERS,
      });
      if (typeof selected === "string") {
        setCoverSourcePath(selected);
      }
    } catch (err) {
      setError(describeError(err));
    }
  }

  async function confirmImport() {
    if (probe === null || saving) {
      return;
    }

    setSaving(true);
    setError(null);
    try {
      if (probe.kind === "exe") {
        await gameApi.addGameFromExe(
          probe.data.sourcePath,
          name,
          coverSourcePath ?? undefined,
        );
      } else {
        await gameApi.addGameFromShortcut(
          probe.data.sourcePath,
          name,
          overridePath ?? undefined,
          coverSourcePath ?? undefined,
        );
      }
      onImported();
      onClose();
    } catch (err) {
      // 保留用户输入与预览信息，只提示错误
      setError(describeError(err));
      setSaving(false);
    }
  }

  return (
    <div
      className={styles.overlay}
      onMouseDown={(event) => {
        if (event.target === event.currentTarget && !saving) {
          onClose();
        }
      }}
    >
      <div
        className={styles.panel}
        role="dialog"
        aria-modal="true"
        aria-label="添加游戏"
      >
        <header className={styles.header}>
          <h2 className={styles.title}>添加游戏</h2>
          <button
            className={styles.closeButton}
            type="button"
            onClick={onClose}
            disabled={saving}
            aria-label="关闭"
          >
            ✕
          </button>
        </header>

        <div className={styles.body}>
          {error !== null && (
            <div className={styles.error} role="alert">
              {error}
            </div>
          )}

          {step === "choose" && (
            <div className={styles.entries}>
              <button
                className={styles.entryPrimary}
                type="button"
                onClick={() => void chooseExe()}
              >
                选择 EXE
              </button>
              <button
                className={styles.entry}
                type="button"
                onClick={() => void chooseShortcut()}
              >
                导入快捷方式
              </button>
              <p className={styles.hint}>
                直接选择 .exe，或导入 .lnk 快捷方式（会解析出它指向的目标、启动参数与工作目录）。
              </p>
            </div>
          )}

          {step === "probing" && (
            <p className={styles.hint}>正在读取文件信息…</p>
          )}

          {step === "ready" && probe !== null && (
            <div className={styles.preview}>
              <label className={styles.field}>
                <span className={styles.fieldLabel}>显示名称</span>
                <input
                  ref={nameInputRef}
                  className={styles.input}
                  value={name}
                  onChange={(event) => setName(event.currentTarget.value)}
                  disabled={saving}
                />
              </label>

              <dl className={styles.details}>
                {probe.kind === "shortcut" && (
                  <>
                    <dt className={styles.detailLabel}>快捷方式</dt>
                    <dd className={styles.detailValue}>{probe.data.sourcePath}</dd>
                  </>
                )}

                <dt className={styles.detailLabel}>启动目标</dt>
                <dd className={styles.detailValue}>
                  {overridePath ?? probe.data.launchPath}
                </dd>

                {probe.kind === "exe" && (
                  <>
                    <dt className={styles.detailLabel}>文件大小</dt>
                    <dd className={styles.detailValue}>
                      {formatBytes(probe.data.sizeBytes)}
                    </dd>
                  </>
                )}

                {probe.kind === "shortcut" && probe.data.launchArgs !== "" && (
                  <>
                    <dt className={styles.detailLabel}>启动参数</dt>
                    <dd className={styles.detailValue}>
                      {probe.data.launchArgs}
                    </dd>
                  </>
                )}

                {probe.data.workingDirectory !== "" && (
                  <>
                    <dt className={styles.detailLabel}>工作目录</dt>
                    <dd className={styles.detailValue}>
                      {probe.data.workingDirectory}
                    </dd>
                  </>
                )}
              </dl>

              <div className={styles.coverRow}>
                <span className={styles.fieldLabel}>自定义封面</span>
                {coverSourcePath === null ? (
                  <button
                    className={styles.smallButton}
                    type="button"
                    onClick={() => void chooseCover()}
                    disabled={saving}
                  >
                    选择图片
                  </button>
                ) : (
                  <>
                    <span className={styles.coverPath} title={coverSourcePath}>
                      {coverSourcePath}
                    </span>
                    <button
                      className={styles.smallButton}
                      type="button"
                      onClick={() => setCoverSourcePath(null)}
                      disabled={saving}
                    >
                      清除
                    </button>
                  </>
                )}
              </div>
              <p className={styles.hint}>
                不选封面时，会尝试从启动目标提取图标；两者都拿不到就显示占位图。原图只会被复制，不会移动或删除。
              </p>

              {probe.kind === "shortcut" && !probe.data.targetExists && (
                <div className={styles.missing} role="alert">
                  <strong>快捷方式指向的目标不存在</strong>
                  <p className={styles.missingPath}>{probe.data.launchPath}</p>
                  {overridePath === null ? (
                    <>
                      <p className={styles.missingHint}>
                        目标可能已被移动或卸载。你可以手动指定真正的启动目标，或取消导入
                        —— 不会以「可启动」的状态存进库中。
                      </p>
                      <button
                        className={styles.smallButton}
                        type="button"
                        onClick={() => void chooseOverrideTarget()}
                        disabled={saving}
                      >
                        手动选择启动目标
                      </button>
                    </>
                  ) : (
                    <p className={styles.resolved}>
                      已改为：{overridePath}
                    </p>
                  )}
                </div>
              )}

              {probe.kind === "shortcut" && probe.data.isLauncher && (
                <div className={styles.launcher} role="status">
                  <strong>这看起来是一个平台启动器</strong>
                  <p className={styles.launcherHint}>
                    启动器往往会再拉起真正的游戏进程。目前按进程树判定结束时机，
                    对这类目标计时可能不准确。
                  </p>
                </div>
              )}

              {duplicate !== null && (
                <div className={styles.duplicate} role="status">
                  <strong>这个启动目标已经在库中</strong>
                  <p className={styles.duplicateText}>
                    已有记录：{duplicate.name}
                  </p>
                  <p className={styles.duplicateHint}>
                    为避免重复计时，不会再创建一条记录。若要改名字或换封面，请取消后编辑已有记录。
                  </p>
                </div>
              )}
            </div>
          )}
        </div>

        <footer className={styles.footer}>
          {step === "ready" && (
            <button
              className={styles.secondaryButton}
              type="button"
              onClick={resetToChoose}
              disabled={saving}
            >
              重新选择
            </button>
          )}

          <button
            className={styles.ghostButton}
            type="button"
            onClick={onClose}
            disabled={saving}
          >
            取消
          </button>

          {step === "ready" && (
            <button
              className={styles.primaryButton}
              type="button"
              onClick={() => void confirmImport()}
              disabled={saving || blocked || name.trim().length === 0}
            >
              {saving ? "导入中…" : "确认导入"}
            </button>
          )}
        </footer>
      </div>
    </div>
  );
}
