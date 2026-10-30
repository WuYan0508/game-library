/**
 * 手柄控制服务（层 1：游戏库导航）。
 *
 * 基于标准 Gamepad API（WebView2/Chromium 原生支持，无需驱动与 Rust 改动）。
 * - 方向：十字键（12-15）或左摇杆（死区 0.5），支持长按重复（首延迟 420ms，连发 130ms）
 * - 按键：A=确认启动，B=返回，Y=详情，菜单=设置（边缘触发，长按不重复）
 * - 安全阀：仅当「手柄控制」开启、有已连接手柄、且窗口持有焦点时才轮询；
 *   启动游戏抢走焦点后自动挂起，不会在后台移动游戏库焦点。
 */

export type GamepadAction =
  | "up"
  | "down"
  | "left"
  | "right"
  | "confirm"
  | "back"
  | "detail"
  | "menu";

const STORAGE_KEY = "gamepadControl";
/** 左摇杆死区。 */
const DEADZONE = 0.5;
/** 方向长按首次重复前的延迟。 */
const REPEAT_DELAY = 450;
/** 方向长按的连发间隔。 */
const REPEAT_INTERVAL = 150;
/** 方向切换的边缘冷却：吸收十字键抖动，避免一次按键连跳多格。 */
const EDGE_COOLDOWN = 90;
/** 手柄停止输入多久后视为闲置（提示条隐藏）。 */
const GAMEPAD_IDLE_MS = 3000;

type Handler = (action: GamepadAction) => void;

/** 提示条的单个按键提示。 */
export interface GamepadHint {
  /** 按键徽标文字，如 "A"、"Y"、"≡"。 */
  key: string;
  /** 动作说明，如 "启动"。 */
  label: string;
}

/** 手柄处理器分层栈：后注册的在最上层，只有栈顶接收操作。
 *  用稳定 id 注册（如 "library-grid"、"dialog-detail"）——
 *  更新同 id 只替换处理器与提示、不改变层级，避免重渲染时层级乱跳。
 *  每层携带自己的提示条文案，提示条组件跟随栈顶层显示。 */
const stack: { id: string; handler: Handler; hints: GamepadHint[] }[] = [];

let connectedState = false;
/** 手柄最近一次输入的时间戳（performance.now）。 */
let lastGamepadAt = 0;
/** 鼠标 / 键盘最近一次输入的时间戳（用于判断用户是否切回了键鼠）。 */
let lastExternalAt = 0;
/** 手柄处于活跃操作状态（有输入且未闲置超时）。 */
let padActive = false;
const stateHandlers = new Set<() => void>();

function notifyState(): void {
  stateHandlers.forEach((handler) => handler());
}
// 模块加载即从存储初始化：否则重启后开关显示开着、轮询却永远不会启动
let enabled = localStorage.getItem(STORAGE_KEY) === "true";
let rafId: number | null = null;
let heldDir: GamepadAction | null = null;
let nextRepeat = 0;
let lastMoveAt = 0;
let prevButtons: boolean[] = [];

export function getStoredGamepadEnabled(): boolean {
  return localStorage.getItem(STORAGE_KEY) === "true"; // 默认关闭
}

/** 当前连接的手柄名（取第一个）；没有则为 null。 */
export function getConnectedGamepadName(): string | null {
  const pads = Array.from(navigator.getGamepads?.() ?? []).filter(
    (pad): pad is Gamepad => pad !== null,
  );
  if (pads.length === 0) {
    return null;
  }
  const id = pads[0].id.trim();
  return id.length > 0 ? id : null;
}

/**
 * 注册/更新/移除一层手柄处理器。
 * 传 handler = 注册（新 id 压栈顶，已有 id 原位替换）；传 null = 移除该层。
 */
export function setGamepadLayer(
  id: string,
  handler: Handler | null,
  hints: GamepadHint[] = [],
): void {
  const index = stack.findIndex((entry) => entry.id === id);
  if (handler === null) {
    if (index >= 0) {
      stack.splice(index, 1);
    }
  } else if (index >= 0) {
    stack[index].handler = handler;
    stack[index].hints = hints;
  } else {
    stack.push({ id, handler, hints });
  }
  if (stack.length > 0) {
    start();
  } else {
    stop();
  }
  notifyState();
}

export function setGamepadEnabled(value: boolean): void {
  localStorage.setItem(STORAGE_KEY, value ? "true" : "false");
  enabled = value;
  if (!enabled) {
    connectedState = false;
    padActive = false;
    stop();
  } else {
    start();
  }
  notifyState();
}

/** 当前手柄状态快照（供提示条组件渲染）。 */
export interface GamepadState {
  enabled: boolean;
  connected: boolean;
  /** 手柄正在被操作（有输入且未闲置超时）——提示条据此显隐。 */
  active: boolean;
  /** 栈顶层的提示文案；空栈为 null。 */
  hints: GamepadHint[] | null;
}

export function getGamepadState(): GamepadState {
  const top = stack.length > 0 ? stack[stack.length - 1] : null;
  return {
    enabled,
    connected: connectedState,
    active: padActive,
    hints: top ? top.hints : null,
  };
}

/** 订阅手柄状态变化（启用开关 / 连接与断开 / 层切换）。 */
export function onGamepadState(handler: () => void): () => void {
  stateHandlers.add(handler);
  return () => stateHandlers.delete(handler);
}

function emit(action: GamepadAction): void {
  if (stack.length === 0) {
    return;
  }
  lastGamepadAt = performance.now();
  if (!padActive) {
    padActive = true;
    notifyState();
  }
  stack[stack.length - 1].handler(action);
}

let externalListening = false;

/** 键鼠输入标记：用户切回键鼠后提示条自动隐藏。 */
function listenExternalInput(): void {
  if (externalListening) {
    return;
  }
  externalListening = true;
  const mark = () => {
    lastExternalAt = performance.now();
  };
  window.addEventListener("pointermove", mark, { passive: true });
  window.addEventListener("pointerdown", mark, { passive: true });
  window.addEventListener("keydown", mark, { passive: true });
  window.addEventListener("wheel", mark, { passive: true });
}

function start(): void {
  if (rafId !== null || !enabled || stack.length === 0) {
    return;
  }
  listenExternalInput();
  rafId = requestAnimationFrame(poll);
}

function stop(): void {
  if (rafId !== null) {
    cancelAnimationFrame(rafId);
    rafId = null;
  }
  heldDir = null;
  prevButtons = [];
}

function poll(): void {
  rafId = requestAnimationFrame(poll);

  // 窗口失焦即挂起：启动的游戏里按 A 不会移动游戏库焦点
  if (document.hidden || !document.hasFocus()) {
    heldDir = null;
    prevButtons = [];
    return;
  }

  const pads = Array.from(navigator.getGamepads?.() ?? []).filter(
    (pad): pad is Gamepad => pad !== null,
  );
  const now = performance.now();
  const connectedNow = pads.length > 0;
  if (connectedNow !== connectedState) {
    connectedState = connectedNow;
    notifyState();
  }
  if (pads.length === 0) {
    heldDir = null;
    prevButtons = [];
    if (padActive) {
      padActive = false;
      notifyState();
    }
    return;
  }
  const pad = pads[0];
  const padActiveNow =
    lastGamepadAt >= lastExternalAt && now - lastGamepadAt < GAMEPAD_IDLE_MS;
  if (padActiveNow !== padActive) {
    padActive = padActiveNow;
    notifyState();
  }

  // 方向：十字键优先，左摇杆叠加（死区 0.5）
  const ax = pad.axes[0] ?? 0;
  const ay = pad.axes[1] ?? 0;
  const pressed = (index: number) => pad.buttons[index]?.pressed ?? false;
  let dir: GamepadAction | null = null;
  if (pressed(14) || ax <= -DEADZONE) dir = "left";
  else if (pressed(15) || ax >= DEADZONE) dir = "right";
  else if (pressed(12) || ay <= -DEADZONE) dir = "up";
  else if (pressed(13) || ay >= DEADZONE) dir = "down";

  if (dir !== null) {
    const isEdge = dir !== heldDir;
    if (isEdge && now - lastMoveAt < EDGE_COOLDOWN) {
      // 冷却期内的方向切换视为抖动：更新方向但不发射
      heldDir = dir;
      nextRepeat = now + REPEAT_DELAY;
    } else if (isEdge) {
      heldDir = dir;
      nextRepeat = now + REPEAT_DELAY;
      lastMoveAt = now;
      emit(dir);
    } else if (!isEdge && now >= nextRepeat) {
      nextRepeat = now + REPEAT_INTERVAL;
      lastMoveAt = now;
      emit(dir);
    }
  } else {
    heldDir = null;
  }

  // 按键边缘触发（standard mapping：0=A，1=B，3=Y，9=菜单）
  if (pressed(0) && !prevButtons[0]) emit("confirm");
  if (pressed(1) && !prevButtons[1]) emit("back");
  if (pressed(3) && !prevButtons[3]) emit("detail");
  if (pressed(9) && !prevButtons[9]) emit("menu");
  prevButtons = pad.buttons.map((button) => button.pressed);
}
