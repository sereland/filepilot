import assert from "node:assert/strict";
import { readFile } from "node:fs/promises";
import test from "node:test";
import ts from "typescript";
async function load(source) {
  const text=await readFile(new URL(source,import.meta.url),"utf8");
  const {outputText}=ts.transpileModule(text,{compilerOptions:{module:ts.ModuleKind.ESNext,target:ts.ScriptTarget.ES2022}});
  return import("data:text/javascript;base64,"+Buffer.from(outputText).toString("base64"));
}
const {toQuery,fromQuery}=await load("../src/lib/conditionQuery.ts");
const {validateRule}=await load("../src/lib/ruleValidation.ts");
const {directoryConditions}=await load("../src/lib/types.ts");
const {itemCountLabel,uniqueFileCount}=await load("../src/lib/ui.ts");
const example=JSON.parse(await readFile(new URL("./fixtures/invoice-rule.json",import.meta.url),"utf8"));
test("invoice fixture survives query builder roundtrip with OR, AND and exclusion",()=>{
  assert.deepEqual(fromQuery(toQuery(example.conditions)),example.conditions);
  assert.deepEqual(validateRule(example),{});
});
test("legacy conditions retain their meaning when converted to an editable group",()=>{
  const old=[{type:"extension",exts:["pdf","docx"]},{type:"name_contains",keyword:"账单"},{type:"size_greater_than",bytes:1048576},{type:"created_within_days",days:7}];
  const updated=fromQuery(toQuery(old));
  assert.equal(updated[0].mode,"all");assert.equal(updated[0].conditions[1].keyword,"账单");assert.equal(updated[0].conditions[2].bytes,1048576);assert.equal(updated[0].conditions[3].field,"created");
});
test("size interval and negated groups roundtrip without turning zero into a missing value",()=>{
  const conditions=[{type:"group",mode:"any",negate:true,conditions:[{type:"size_between",min:0,max:1048576},{type:"size_less_than",bytes:100},{type:"date_match",field:"modified",operator:"older_than_days",days:30}]}];
  assert.deepEqual(fromQuery(toQuery(conditions)),conditions);
});
test("empty groups, missing values and invalid intervals are rejected before save",()=>{
  for(const condition of [{type:"group",mode:"all",negate:true,conditions:[]},{type:"name_match",operator:"starts_with",keyword:" "},{type:"size_between",min:2,max:1},{type:"date_match",field:"modified",operator:"within_days",days:0}]) {
    assert.ok(Object.keys(validateRule({...example,conditions:[condition]})).some(key=>key.startsWith("condition")));
  }
  const missing=fromQuery({combinator:"and",rules:[{field:"size",operator:"between",value:[0,""]}]});
  assert.ok(Object.keys(validateRule({...example,conditions:missing})).some(key=>key.startsWith("condition")));
});
test("changing to directories removes incompatible fields even inside nested groups",()=>{
  const conditions=directoryConditions(example.conditions);
  assert.equal(conditions[0].conditions[0].type,"name_contains");
  assert.ok(Object.keys(validateRule({...example,target_kind:"directory",actions:[example.actions[0]],conditions})).some(key=>key.startsWith("condition")));
});
test("path traversal and malformed templates are rejected while valid relative templates remain usable",()=>{
  for(const subfolder of ["../escape","/absolute","C:/absolute","{unknown}","{year"]) {
    assert.ok(validateRule({...example,actions:[{...example.actions[1],subfolder}]})["action-0"]);
  }
  assert.deepEqual(validateRule({...example,actions:[{...example.actions[1],subfolder:"{year}/{month}"}]}),{});
  assert.ok(validateRule({...example,actions:[{type:"rename",pattern:"../escape.pdf"}]})["action-0"]);
});
test("recycle must be the final action and chain records count as a single file",()=>{
  assert.ok(validateRule({...example,actions:[{type:"move_to_recycle_bin"},example.actions[1]]})["action-0"]);
  const records=[{src:"C:/a.pdf",origin_src:"C:/a.pdf"},{src:"C:/new-a.pdf",origin_src:"c:\\a.pdf"}];
  assert.equal(itemCountLabel(records),"1 个文件");assert.equal(uniqueFileCount(records),1);
});
test("regex name conditions roundtrip and expose only non-excluded named captures to templates",async()=>{
  const {captureVariables}=await load("../src/lib/ruleValidation.ts");
  const conditions=[{type:"group",mode:"all",negate:false,conditions:[{type:"name_match",operator:"regex",keyword:"^报价_(?<客户>[^_]+)_(?P<年>\\d{4})"},{type:"group",mode:"any",negate:true,conditions:[{type:"name_match",operator:"regex",keyword:"_(?<状态>草稿)"}]}]}];
  assert.deepEqual(fromQuery(toQuery(conditions)),conditions);
  assert.deepEqual(captureVariables(conditions),["客户","年"]);
  const rule={...example,conditions,actions:[{type:"organize",operation:"move",dest:"D:\\工作",subfolder:"{客户}/{年}",date_source:"now"},{type:"rename",pattern:"{客户}_{name}.{ext}"}]};
  assert.deepEqual(validateRule(rule),{});
  assert.ok(validateRule({...rule,actions:[{...rule.actions[0],subfolder:"{状态}"}]})["action-0"]);
  assert.ok(validateRule({...rule,actions:[{...rule.actions[0],subfolder:"{项目}"}]})["action-0"]);
  for(const keyword of [" ","(?<name>.+)","a".repeat(501)]) {
    assert.ok(Object.keys(validateRule({...example,conditions:[{type:"name_match",operator:"regex",keyword}]})).some(key=>key.startsWith("condition")),keyword);
  }
});
test("live summary describes empty fields inside condition groups with placeholders",async()=>{
  const js=async source=>ts.transpileModule(await readFile(new URL(source,import.meta.url),"utf8"),{compilerOptions:{module:ts.ModuleKind.ESNext,target:ts.ScriptTarget.ES2022}}).outputText;
  const toUrl=text=>"data:text/javascript;base64,"+Buffer.from(text).toString("base64");
  const summary=(await js("../src/lib/ruleSummary.ts")).replace(/(["'])\.\/types\1/g,JSON.stringify(toUrl(await js("../src/lib/types.ts"))));
  const {summarizeRule,normalizeRule,autoRuleName}=await import(toUrl(summary));
  const base={id:"",name:"",enabled:false,target_kind:"file",watch_folders:[" "],actions:[{type:"move",dest:""}]};
  const empty=summarizeRule({...base,conditions:fromQuery(toQuery([{type:"extension",exts:[]}]))});
  assert.ok(!empty.includes("扩展名为 ）"),empty);
  assert.equal(empty,"在 来源文件夹 中，扩展名是所选文件类型 的文件，将移动到目标文件夹。仅手动执行。");
  const filled=summarizeRule({...base,watch_folders:["D:\\下载"],conditions:[{type:"group",mode:"any",negate:false,conditions:[{type:"extension",exts:["xlsx"]},{type:"name_match",operator:"regex",keyword:""}]},{type:"group",mode:"all",negate:true,conditions:[]}]});
  assert.equal(filled,"在 D:\\下载 中，（扩展名为 xlsx 或 文件名符合指定条件） 的文件，将移动到目标文件夹。仅手动执行。");
  assert.deepEqual(normalizeRule({...base,name:" 报价 ",watch_folders:[" D:\\a ","D:\\a",""],conditions:[],actions:[{type:"copy",dest:" E:\\b "}]}),{...base,name:"报价",watch_folders:["D:\\a"],conditions:[],actions:[{type:"copy",dest:"E:\\b"}]});
  const quotes={...base,conditions:[{type:"extension",exts:["xlsx","xls"]},{type:"group",mode:"all",negate:true,conditions:[{type:"extension",exts:["tmp"]}]}],actions:[{type:"organize",operation:"move",dest:"D:\\工作\\报价\\",subfolder:"{客户}",date_source:"now"}]};
  assert.equal(autoRuleName(quotes),"XLSX/XLS 文件按客户归档到报价");
  assert.equal(autoRuleName({...base,conditions:[],actions:[{type:"organize",operation:"copy",dest:"",subfolder:"{year}/{month}",date_source:"now"}]}),"文件按年月复制");
  assert.equal(autoRuleName({...base,target_kind:"directory",conditions:[],actions:[{type:"move_to_recycle_bin"}]}),"文件夹清理");
});
