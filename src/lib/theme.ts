// 主题管理逻辑

export type Theme = 'auto' | 'light' | 'dark';

const STORAGE_KEY = 'filepilot-theme';

// 获取系统主题
export function getSystemTheme(): 'light' | 'dark' {
  return window.matchMedia('(prefers-color-scheme: dark)').matches ? 'dark' : 'light';
}

// 获取当前生效的主题（考虑 auto 模式）
export function getEffectiveTheme(theme: Theme): 'light' | 'dark' {
  if (theme === 'auto') {
    return getSystemTheme();
  }
  return theme;
}

// 应用主题到 DOM
export function applyTheme(theme: 'light' | 'dark') {
  document.documentElement.setAttribute('data-theme', theme);
}

// 从 localStorage 加载用户主题设置
export function loadTheme(): Theme {
  const stored = localStorage.getItem(STORAGE_KEY);
  if (stored === 'light' || stored === 'dark' || stored === 'auto') {
    return stored;
  }
  return 'auto'; // 默认跟随系统
}

// 保存主题设置到 localStorage
export function saveTheme(theme: Theme) {
  localStorage.setItem(STORAGE_KEY, theme);
}

// 监听系统主题变化
export function watchSystemTheme(callback: (theme: 'light' | 'dark') => void) {
  const media = window.matchMedia('(prefers-color-scheme: dark)');

  const listener = (e: MediaQueryListEvent) => {
    callback(e.matches ? 'dark' : 'light');
  };

  media.addEventListener('change', listener);

  // 返回清理函数
  return () => media.removeEventListener('change', listener);
}

// 初始化主题系统
export function initTheme() {
  const userTheme = loadTheme();
  const effectiveTheme = getEffectiveTheme(userTheme);
  applyTheme(effectiveTheme);

  // 当前选择可能在设置页改变，事件到来时读取最新偏好。
  return watchSystemTheme((systemTheme) => {
    if (loadTheme() === 'auto') applyTheme(systemTheme);
  });
}
