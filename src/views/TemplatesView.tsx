import { IconFile, IconFolder, IconImage } from "../components/icons";
import { RULE_TEMPLATES, ruleFromTemplate } from "../lib/templates";
import type { Rule } from "../lib/types";

export default function TemplatesView({ onUse }: { onUse: (rule: Rule) => void }) {
  return <div>
    <header className="page-header"><div><h1>规则模板</h1><p className="page-sub">选择模板，设置来源和目标文件夹，保存后先预览。</p></div></header>
    <div className="template-gallery">{RULE_TEMPLATES.map((template) => <article className="template-card" key={template.key}>
      {template.icon === "image" ? <IconImage /> : template.icon === "file" ? <IconFile /> : <IconFolder />}
      <h2>{template.name}</h2><p>{template.description}</p><div className="template-logic">{template.summary}</div>
      <button className="btn btn-secondary" onClick={() => onUse(ruleFromTemplate(template.key))}>使用模板 <span aria-hidden="true">→</span></button>
    </article>)}</div>
  </div>;
}
