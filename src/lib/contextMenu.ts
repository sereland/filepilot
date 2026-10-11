// 保留文本编辑菜单，屏蔽桌面 WebView 的刷新、打印等浏览器菜单。
export function preventAppContextMenu(event: MouseEvent) {
  if (event.target instanceof Element) {
    const field = event.target.closest("input, textarea");
    if (field instanceof HTMLTextAreaElement) return;
    if (field instanceof HTMLInputElement && ["text", "search", "password", "email", "url", "tel", "number"].includes(field.type)) return;
    if (event.target instanceof HTMLElement && event.target.isContentEditable) return;
  }
  event.preventDefault();
}
