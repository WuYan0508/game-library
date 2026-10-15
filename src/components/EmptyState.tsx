import styles from "./EmptyState.module.css";

interface EmptyStateProps {
  title?: string;
  description?: string;
  actionLabel?: string;
  /** 按钮暂不可用时的说明。 */
  actionHint?: string;
  /** 提供后按钮可用；缺省时按钮为禁用态并显示 actionHint。 */
  onAction?: () => void;
}

export default function EmptyState({
  title = "游戏库还是空的",
  description = "导入本机的游戏后，这里会显示封面、名称和累计游玩时长。只有通过本应用启动的游戏才会被计时。",
  actionLabel = "添加第一款游戏",
  actionHint = "点击上方「添加游戏」也可以导入",
  onAction,
}: EmptyStateProps) {
  const enabled = onAction !== undefined;

  return (
    <div className={styles.empty}>
      <div className={styles.icon} aria-hidden="true">
        <svg
          viewBox="0 0 48 48"
          width="48"
          height="48"
          fill="none"
          stroke="currentColor"
          strokeWidth="1.5"
          strokeLinecap="round"
          strokeLinejoin="round"
        >
          <rect x="6" y="13" width="36" height="24" rx="4" />
          <path d="M17 25h6M20 22v6" />
          <circle cx="31" cy="23" r="1.7" fill="currentColor" stroke="none" />
          <circle cx="34.5" cy="27" r="1.7" fill="currentColor" stroke="none" />
        </svg>
      </div>

      <h2 className={styles.title}>{title}</h2>
      <p className={styles.description}>{description}</p>

      <button
        className={styles.action}
        type="button"
        disabled={!enabled}
        title={enabled ? undefined : actionHint}
        onClick={onAction}
      >
        {actionLabel}
      </button>
      {!enabled && <p className={styles.hint}>{actionHint}</p>}
    </div>
  );
}
