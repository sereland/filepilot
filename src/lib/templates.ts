import { newEmptyRule, type Rule } from "./types";

export const RULE_TEMPLATES = [
  { key: "images", name: "图片归档", icon: "image", description: "把下载的图片集中收纳，找图时少翻几个文件夹。", summary: ".png / .jpg / .jpeg → 移动到图片文件夹" },
  { key: "pdf", name: "PDF 文档整理", icon: "file", description: "让下载的报告、资料和使用手册有一个固定去处。", summary: ".pdf → 移动到文档文件夹" },
  { key: "invoice", name: "发票报销", icon: "folder", description: "复制发票到待报销文件夹，原文件保留在来源位置。", summary: "文件名包含「发票」 → 复制到待报销文件夹" },
] as const;

export function ruleFromTemplate(key: typeof RULE_TEMPLATES[number]["key"]): Rule {
  return {
    ...newEmptyRule(),
    name: RULE_TEMPLATES.find((template) => template.key === key)!.name,
    conditions: key === "invoice" ? [{ type: "name_contains", keyword: "发票" }] : [{ type: "extension", exts: key === "pdf" ? ["pdf"] : ["png", "jpg", "jpeg"] }],
    actions: [{ type: key === "invoice" ? "copy" : "move", dest: "" }],
  };
}
