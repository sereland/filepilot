# FilePilot 设计规范 v2.0

> 极简商务风 - 克制、专业、易用

---

## 📐 设计原则

### 1. 克制优先
- 单一品牌色（蓝色），不使用多色系统
- 扁平化设计，避免过度装饰
- 静态为主，动效克制（150ms 过渡）
- 无浮动效果、无缩放动画

### 2. 信息清晰
- 清晰的视觉层级（标题/正文/辅助文字）
- 合理的间距节奏（16/24/32）
- 单列布局，垂直扫视
- 如果/那么 的自然语言表述

### 3. 专业可信
- 轻微阴影增加层次感
- 一致的圆角和间距
- Inter 字体（现代、易读）
- 响应式布局适配

---

## 🎨 色彩系统

### 浅色模式（Light Mode）

```css
/* 背景 */
--bg: #ffffff;              /* 页面背景 */
--bg-secondary: #fafafa;    /* 侧边栏、卡片区域背景 */
--bg-hover: #f5f5f5;        /* hover 状态 */

/* 文字 */
--text-primary: #111827;    /* 标题、主要文字 */
--text-secondary: #6b7280;  /* 正文、描述 */
--text-tertiary: #9ca3af;   /* 辅助文字、禁用状态 */

/* 边框 */
--border: #e5e7eb;          /* 默认边框 */
--border-hover: #d1d5db;    /* hover 边框 */

/* 品牌色 */
--brand: #2563eb;           /* 主品牌色（蓝色）*/
--brand-hover: #1d4ed8;     /* hover 状态 */
--brand-light: #eff6ff;     /* 浅色背景（选中状态）*/

/* 语义色 */
--success: #10b981;         /* 成功（保留但尽量少用）*/
--warning: #f59e0b;         /* 警告 */
--danger: #ef4444;          /* 危险操作 */
```

### 暗黑模式（Dark Mode）

```css
/* 背景 */
--bg: #0a0a0a;              /* 页面背景 */
--bg-secondary: #141414;    /* 侧边栏、卡片区域背景 */
--bg-hover: #1a1a1a;        /* hover 状态 */

/* 文字 */
--text-primary: #f5f5f5;    /* 标题、主要文字 */
--text-secondary: #a1a1a1;  /* 正文、描述 */
--text-tertiary: #737373;   /* 辅助文字、禁用状态 */

/* 边框 */
--border: #262626;          /* 默认边框 */
--border-hover: #404040;    /* hover 边框 */

/* 品牌色 */
--brand: #3b82f6;           /* 主品牌色（稍微调亮）*/
--brand-hover: #60a5fa;     /* hover 状态 */
--brand-light: #1e3a5f;     /* 浅色背景（选中状态）*/

/* 语义色 */
--success: #22c55e;         /* 成功 */
--warning: #f59e0b;         /* 警告 */
--danger: #ef4444;          /* 危险操作 */
```

### 色彩使用规则

| 元素 | 颜色 | 说明 |
|------|------|------|
| 主按钮 | `--brand` | 新建规则、确定等主要操作 |
| Toggle 开关（激活）| `--brand` | 统一用品牌色 |
| 监控状态圆点 | `--brand` | 统一用品牌色 |
| 导航选中状态 | `--brand-light` + `--brand` 文字 | 浅色背景 + 品牌色文字 |
| 编辑按钮 | `--brand` | 最常用的操作，用品牌色突出 |
| 删除按钮 hover | `--danger` | 危险操作用红色警示 |
| 其他文字按钮 | `--text-tertiary` → `--text-primary` | 默认灰色，hover 变黑 |

---

## 📏 间距系统

### 间距比例（4px 基准）

```css
--space-xs: 8px;    /* 小间距：图标和文字 */
--space-sm: 12px;   /* 中小间距：导航项内边距 */
--space-md: 16px;   /* 中间距：卡片之间、分组 */
--space-lg: 24px;   /* 大间距：卡片内边距、页面分区 */
--space-xl: 32px;   /* 超大间距：页面边距 */
```

### 具体应用

| 元素 | 间距 |
|------|------|
| 页面 padding | 32px (xl) |
| 卡片 padding | 24px (lg) |
| 卡片之间 gap | 16px (md) |
| 按钮内 padding | 8px 16px |
| 导航项 padding | 8px 12px |
| 图标和文字 gap | 8px (xs) |

---

## 🔤 字体系统

### 字体家族

```css
font-family: 'Inter', -apple-system, BlinkMacSystemFont, 'Segoe UI', 
             'Microsoft YaHei', 'PingFang SC', sans-serif;
```

**备用方案**：
- macOS/iOS: `-apple-system`
- Windows: `Segoe UI` / `Microsoft YaHei`
- 中文: `PingFang SC` / `Microsoft YaHei`

### 字阶系统

| 用途 | 大小 | 字重 | 行高 | 说明 |
|------|------|------|------|------|
| 页面标题 | 24px | 700 | 1.25 | 整理规则、操作记录 |
| 副标题 | 13px | 400 | 1.6 | 页面说明文字 |
| 卡片标题 | 15px | 600 | 1.3 | 规则名称 |
| 正文 | 14px | 400 | 1.6 | 默认字体大小 |
| 按钮文字 | 14px | 600 | - | 主按钮 |
| 链接按钮 | 13px | 500 | - | 编辑、预览、删除 |
| 辅助文字 | 13px | 400 | 1.6 | 文件夹路径、时间 |
| 小字 | 12px | 400 | 1.5 | 监控统计、规则标签 |

---

## 📦 组件规范

### 侧边栏（Sidebar）

```
宽度: 240px
背景: --bg
边框: 1px solid --border (右侧)
Padding: 20px 12px
```

**结构**：
1. 品牌区（Brand）
   - Logo + 名称
   - Padding: 8px 12px 24px
2. 导航区（Nav）
   - 导航项高度: 36px
   - 圆角: 6px
   - Gap: 2px
3. 弹性空间（Spacer）
4. 监控卡（Monitor Card）
   - 圆角: 8px
   - 背景: --bg-secondary
   - 边框: 1px solid --border
   - 无阴影

### 主内容区（Main Content）

```
背景: --bg-secondary
Padding: 32px 40px
```

**页面头部**：
- 标题 + 副标题
- 主按钮右对齐
- 底部 border: 1px solid --border
- Padding-bottom: 16px
- Margin-bottom: 24px

### 规则卡片（Rule Card）

```
布局: 单列
间距: 16px
圆角: 8px
Padding: 24px
阴影: 0 1px 3px rgba(0, 0, 0, 0.08)
Hover: 
  - 边框变色 (--border-hover)
  - 阴影加深 (0 2px 8px rgba(0, 0, 0, 0.1))
  - 无浮动效果
```

**卡片结构**：
```
┌─────────────────────────────────┐
│ [规则名称]            [Toggle]   │  ← Header (flex justify-between)
│ 📁 文件夹路径                     │  ← Folder (12px margin-bottom)
│ ─────────────────────────────    │  ← Border-top
│ 如果: 文件类型是 .png / .jpg      │  ← 规则逻辑（12px padding-top）
│ 那么: 移动到 图片\                │
│ ─────────────────────────────    │  ← Border-top
│ [编辑] [预览] [日志] [删除]       │  ← Actions (12px padding-top)
└─────────────────────────────────┘
```

**停用状态**：
```css
opacity: 0.5;
```

### Toggle 开关

```
尺寸: 40px × 22px
圆角: 11px (完全圆润)
背景:
  - 未激活: --border
  - 激活: --brand
过渡: 200ms ease
```

**滑块**：
```
尺寸: 18px × 18px
位置:
  - 未激活: left 2px
  - 激活: left 20px
阴影: 0 1px 3px rgba(0, 0, 0, 0.08)
```

### 按钮系统

#### 主按钮（Primary Button）

```css
background: --brand;
color: white;
padding: 8px 16px;
border-radius: 6px;
font-size: 14px;
font-weight: 600;
transition: background 0.15s ease;

hover: background: --brand-hover;
```

#### 文字按钮（Text Link Button）

```css
background: none;
border: none;
color: --text-tertiary;
font-size: 13px;
font-weight: 500;
padding: 0;
transition: color 0.15s ease;

hover: color: --text-primary;
```

**变体**：
- `.primary`: color: --brand (编辑按钮)
- `.danger`: hover 时 color: --danger (删除按钮)

### 导航项（Nav Item）

```css
padding: 8px 12px;
border-radius: 6px;
font-size: 14px;
font-weight: 500;
transition: all 0.15s ease;

default: color: --text-secondary;
hover: 
  background: --bg-hover;
  color: --text-primary;
active:
  background: --brand-light;
  color: --brand;
  font-weight: 600;
```

---

## 🎭 交互规范

### 动画时长

所有过渡动画统一使用 **150ms** 或 **200ms**（开关）：

```css
transition: all 0.15s ease;
transition: background 0.2s ease; /* Toggle 开关 */
```

### Hover 状态

| 元素 | 变化 | 禁止 |
|------|------|------|
| 卡片 | 边框变色 + 阴影加深 | ❌ 浮动、缩放 |
| 主按钮 | 背景变深 | ❌ 浮动、缩放 |
| 文字按钮 | 颜色变深 | ❌ 下划线 |
| Toggle | - | ❌ 任何变化 |
| 导航项 | 背景变色 | ❌ 位移 |

### 焦点状态

```css
outline: 2px solid --brand;
outline-offset: 2px;
border-radius: 4px;
```

### 禁用状态

```css
opacity: 0.5;
cursor: not-allowed;
pointer-events: none;
```

---

## 📱 响应式规范

### 断点

```css
/* 大屏（默认）*/
min-width: 1200px

/* 中屏 */
max-width: 1199px

/* 小屏 */
max-width: 768px
```

### 适配策略

- **侧边栏**: 小屏时隐藏，改为顶部导航栏
- **规则卡片**: 始终单列（不需要响应式）
- **间距**: 小屏时页面 padding 减少到 20px

---

## 🌓 主题切换

### 实现方式

```tsx
// 1. 主题类型
type Theme = 'auto' | 'light' | 'dark';

// 2. 监听系统主题
const systemTheme = window.matchMedia('(prefers-color-scheme: dark)');

// 3. 应用主题
document.documentElement.setAttribute('data-theme', 'light' | 'dark');

// 4. 保存用户选择
localStorage.setItem('theme', theme);
```

### CSS 实现

```css
/* 默认浅色模式 */
:root {
  --bg: #ffffff;
  /* ... */
}

/* 暗黑模式 */
[data-theme="dark"] {
  --bg: #0a0a0a;
  /* ... */
}
```

### 设置界面

```
外观设置
├─ ○ 跟随系统（默认）
├─ ○ 浅色模式
└─ ○ 深色模式
```

---

## 🎯 空状态设计

### 无规则时

```
┌─────────────────────────────────┐
│         [📋 图标]                │
│                                   │
│       还没有规则                  │
│   点击「新建规则」创建第一条       │
│                                   │
│     [新建规则] 按钮                │
└─────────────────────────────────┘
```

样式：
- 居中对齐
- 图标: 80px × 80px，灰色背景圆角
- 标题: 20px font-weight 600
- 描述: 14px text-secondary
- 按钮: 主按钮样式

---

## 📋 实现清单

### Phase 1: 浅色模式基础（优先）
- [ ] 建立 CSS 变量系统
- [ ] 侧边栏组件
- [ ] 主内容区布局
- [ ] 规则卡片组件
- [ ] Toggle 开关组件
- [ ] 按钮系统
- [ ] 空状态

### Phase 2: 暗黑模式
- [ ] 暗黑模式色彩变量
- [ ] 主题切换逻辑
- [ ] 设置页面主题选择器
- [ ] localStorage 持久化
- [ ] 系统主题监听

### Phase 3: 细节打磨
- [ ] 响应式适配
- [ ] 焦点状态
- [ ] 无障碍优化
- [ ] 动画性能优化

---

## 🔍 设计决策记录

### 为什么选择单列布局？
- 规则逻辑需要完整阅读，单列更符合垂直扫视习惯
- 用户规则数量不会太多（3-10 条），不需要双列节省空间
- 避免过度设计，不做"网格/列表切换"功能
- 未来扩展性更好（加统计信息、执行历史等）

### 为什么统一用蓝色？
- 极简风格应该只用一个强调色
- 开关的绿色和按钮的蓝色放一起不协调
- Toggle 表示"启用/停用"，用蓝色也说得通
- 参考 Linear / Notion 都是单色系统

### 为什么不加浮动效果？
- 用户明确表示"看起来不舒服"
- 克制的商务风格不需要过度动效
- hover 时边框和阴影变化已经足够

### 为什么间距从 16px 增加到 24px？
- 原设计过于局促
- 更大的间距让界面更透气、更易读
- 符合现代设计的趋势（Notion / Linear 都是大间距）

---

**版本**: v2.0  
**更新时间**: 2026-10-09  
**设计师**: Claude (AI)  
**审核**: @lee
