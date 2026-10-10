<div align="center">

# FilePilot

**让 Windows 的文件自动归位，一次设置、永久整洁。**

*Free, Chinese-first, modern automatic file organizer for Windows.*

**先预览，再动手，可撤销。**

</div>

---

## 这是什么 / What is this

FilePilot 是一个免费、中文优先、界面现代的 Windows 文件自动整理工具：监控你的下载文件夹（或任何文件夹），按你设定的规则自动移动、重命名、归档文件。

FilePilot is a free, Chinese-first, modern automatic file organizer for Windows. Watch folders, define if-then rules, and let your files sort themselves.

## 为什么做 / Why

- **File Juggler** 要 $40–50，英文界面，上世纪的 WinForms UI
- **DropIt** 2017 年就死了，没人维护
- 微软的 Power Automate 对普通人太难，Storage Sense 太蠢——中间地带是空白
- 中文区没有一个现代的文件自动整理工具

## 核心特性 / Features（v1.0 MVP）

- 📁 文件夹实时监控，set-and-forget
- 🧩 可视化 if-then 规则编辑器（扩展名 / 文件名关键词 / 大小 / 日期 → 移动 / 复制 / 重命名 / 回收站）
- 👀 **dry-run 预览**：执行前先看"将要移动 N 个文件"，默认开启
- ↩️ **一键撤销**：每次操作写日志，托盘菜单随时回滚；删除只进回收站
- 🇨🇳 全中文界面 + 中国用户预设规则包（微信文件分拣 / 发票报销 / 截图归档）
- 📦 安装包 + 便携版双分发，纯本地运行，无需联网，无后台

## 路线图 / Roadmap

- [x] Phase 0：项目骨架 + Windows 自动构建
- [ ] Phase 1：Rust 核心（监控 / 规则引擎 / dry-run / 撤销）
- [ ] Phase 2：前端 UI（规则编辑器 / 预览弹窗 / 日志页 / 托盘）
- [ ] Phase 3：v1.0 打包分发
- [ ] Phase 4：内测打磨，V2EX 首发

## 开发 / Development

界面定稿见 [设计规范](design/UI_FINAL.md)，独立预览见 [最终 HTML 预览](design/preview-final.html)。主窗口默认 1120 × 700，采用侧栏导航、单列规则、独立模板页和文件类型多选。新规则默认仅手动，自动整理可按规则开启。

```bash
npm install
npm run tauri dev        # 需要 Rust 工具链 + 系统依赖
```

仅检查前端可运行 `npm run build`。普通浏览器可查看页面和编辑器，真实文件操作、文件夹选择和监控需在 Tauri 桌面应用中使用。

Windows 安装包由 GitHub Actions 自动构建（见 `.github/workflows/build.yml`），每次打 tag 自动发布到 Releases。

## 许可证 / License

MIT — 详见 [LICENSE](LICENSE)。
