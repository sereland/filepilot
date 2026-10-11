import { QueryBuilder, type Field, type RuleGroupType, type RuleGroupTypeAny, type ValueEditorProps, type ValueSelectorProps } from "react-querybuilder";
import { useEffect, useRef, useState } from "react";
import { fromQuery, toQuery } from "../lib/conditionQuery";
import type { Condition } from "../lib/types";
import ExtensionPicker from "./ExtensionPicker";
import SelectField from "./SelectField";

const operators = {
  extension:[{name:"in",label:"是"}],
  name:[{name:"contains",label:"包含"},{name:"doesNotContain",label:"不包含"},{name:"=",label:"等于"},{name:"beginsWith",label:"开头是"},{name:"endsWith",label:"结尾是"}],
  size:[{name:">",label:"大于"},{name:"<",label:"小于"},{name:"between",label:"介于"}],
  date:[{name:"within",label:"最近"},{name:"older",label:"距今至少"}],
};
function ValueEditor(props: ValueEditorProps) {
  const invalid=props.validation === false || (!!props.validation && typeof props.validation === "object" && !props.validation.valid);
  const label = props.field === "name" ? "名称关键词" : props.field === "size" ? "文件大小（MB）" : "天数";
  if (props.field === "extension") return <ExtensionPicker values={Array.isArray(props.value) ? props.value : []} validation={{"aria-invalid":invalid || undefined}} onChange={props.handleOnChange}/>;
  if (props.field === "size" && props.operator === "between") {
    const values=Array.isArray(props.value)?props.value:String(props.value??"").split(",");
    return <div className="inline-fields range-values">{[0,1].map(index=><input key={index} aria-label={index?"大小上限（MB）":"大小下限（MB）"} aria-invalid={invalid || undefined} type="number" min={0} step="any" value={Number.isFinite(Number(values[index])) ? values[index] : ""} disabled={props.disabled} onChange={e=>props.handleOnChange(index?[values[0],e.target.value]:[e.target.value,values[1]])}/>)}<span>MB</span></div>;
  }
  return <div className="inline-fields"><input aria-label={label} aria-invalid={invalid || undefined} type={props.field === "name" ? "text" : "number"} min={props.field === "size" ? 0 : 1} step={props.field === "size" ? "any" : 1} value={props.value??""} disabled={props.disabled} placeholder={props.field === "name" ? "例如：发票" : undefined} onChange={e=>props.handleOnChange(e.target.value)}/>{props.field !== "name" && <span>{props.field === "size" ? "MB" : "天"}</span>}</div>;
}
export default function ConditionEditor({ conditions, isDirectory, submitted, onChange }: { conditions:Condition[]; isDirectory:boolean; submitted:boolean; onChange:(conditions:Condition[])=>void }) {
  // Keep the editor's raw values and IDs while typing (e.g. "0." MB).
  // Rebuild only when the parent changes conditions outside this editor.
  const [query,setQuery]=useState(()=>toQuery(conditions));
  const emitted=useRef(conditions);
  useEffect(()=>{if(conditions!==emitted.current){emitted.current=conditions;setQuery(toQuery(conditions));}},[conditions]);
  const fields:Field[]=[...(!isDirectory?[{name:"extension",label:"扩展名",operators:operators.extension}]:[]),{name:"name",label:isDirectory?"文件夹名称":"文件名称",operators:operators.name},...(!isDirectory?[{name:"size",label:"文件大小",operators:operators.size}]:[]),{name:"created",label:"创建时间",operators:operators.date},{name:"modified",label:"修改时间",operators:operators.date}];
  return <div className="condition-builder">
    <QueryBuilder fields={fields} query={query} onQueryChange={(next:RuleGroupType)=>{setQuery(next);const value=fromQuery(next);emitted.current=value;onChange(value);}} showNotToggle resetOnFieldChange resetOnOperatorChange autoSelectField autoSelectOperator listsAsArrays
      getDefaultValue={rule=>rule.field === "extension" ? [] : rule.field === "name" ? "" : rule.field === "size" ? rule.operator === "between" ? [0,100] : 100 : 7}
      controlElements={{valueEditor:ValueEditor,valueSelector:Selector}}
      validator={submitted ? validateQuery : undefined}
      translations={{addRule:{label:"＋ 条件",title:"添加条件"},addGroup:{label:"＋ 条件组",title:"添加条件组"},removeRule:{label:"×",title:"删除条件"},removeGroup:{label:"×",title:"删除条件组"},notToggle:{label:"排除",title:"排除此组条件"},combinators:{title:"条件组合"},fields:{title:"条件字段"},operators:{title:"匹配方式"},value:{title:"条件内容"}}}
      combinators={[{name:"and",label:"全部满足"},{name:"or",label:"任一满足"}]} />
    <p className="editor-help">组内可选全部或任一满足，勾选“排除”表示不满足该组。扩展名内任一种均可匹配；名称不区分大小写。</p>
    {hasDate(conditions) && <p className="editor-help">时间条件在文件夹变化或手动预览时检查；静置文件达到天数后，目前需要手动预览整理。</p>}
  </div>;
}

function hasDate(conditions:Condition[]):boolean {return conditions.some(c=>c.type === "date_match" || c.type === "created_within_days" || (c.type === "group" && hasDate(c.conditions)));}

function Selector(props:ValueSelectorProps) {
  type Option={name:string;label:string;disabled?:boolean};
  const options=props.options.flatMap(option=>"options" in option && Array.isArray(option.options) ? option.options as Option[] : [option as Option]);
  const invalid=props.validation === false || (!!props.validation && typeof props.validation === "object" && !props.validation.valid);
  return <SelectField className={props.className} title={props.title} aria-label={props.title} aria-invalid={props.title === "条件组合" && invalid || undefined} value={props.value} disabled={props.disabled} onChange={e=>props.handleOnChange(e.target.value)}>{options.map(option=><option key={option.name} value={option.name} disabled={option.disabled}>{option.label}</option>)}</SelectField>;
}

function validateQuery(query: RuleGroupTypeAny) {
  const map:Record<string,boolean>={};
  const walk=(group:RuleGroupTypeAny,depth=0)=>{
    if(group.id) map[group.id]=group.rules.length>0 && group.rules.length<=100 && depth<=4;
    for(const rule of group.rules) {
      if(typeof rule === "string") continue;
      if("rules" in rule) {walk(rule,depth+1);continue;}
      const valid=rule.field === "extension" ? Array.isArray(rule.value)&&rule.value.length>0 : rule.field === "name" ? !!String(rule.value??"").trim() : rule.operator === "between" ? Array.isArray(rule.value)&&rule.value.length===2&&rule.value.every((v:unknown)=>Number.isFinite(Number(v))&&Number(v)>=0)&&Number(rule.value[0])<=Number(rule.value[1]) : Number.isFinite(Number(rule.value))&&Number(rule.value)>0&&(rule.field === "size" || (Number.isInteger(Number(rule.value))&&Number(rule.value)<=365000));
      if(rule.id) map[rule.id]=valid;
    }
  };walk(query);return map;
}
