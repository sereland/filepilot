import type { RuleGroupType, RuleType } from "react-querybuilder";
import type { Condition } from "./types";

export function toQuery(conditions: Condition[], path = "root"): RuleGroupType {
  const group = conditions.length === 1 && conditions[0].type === "group" ? conditions[0] : null;
  return { id:path, combinator:group?.mode === "any" ? "or" : "and", not:!!group?.negate,
    rules:(group?.conditions ?? conditions).map((condition,i)=> {
      const id=`${path}-${i}`;
      if (condition.type === "group") return toQuery([condition],id);
      switch (condition.type) {
        case "extension": return {id,field:"extension",operator:"in",value:condition.exts};
        case "name_contains": return {id,field:"name",operator:"contains",value:condition.keyword};
        case "name_match": return {id,field:"name",operator:{contains:"contains",not_contains:"doesNotContain",equals:"=",starts_with:"beginsWith",ends_with:"endsWith"}[condition.operator],value:condition.keyword};
        case "size_greater_than": return {id,field:"size",operator:">",value:condition.bytes/1048576};
        case "size_less_than": return {id,field:"size",operator:"<",value:condition.bytes/1048576};
        case "size_between": return {id,field:"size",operator:"between",value:[condition.min/1048576,condition.max/1048576]};
        case "created_within_days": return {id,field:"created",operator:"within",value:condition.days};
        case "date_match": return {id,field:condition.field,operator:condition.operator === "within_days" ? "within" : "older",value:condition.days};
      }
    }) };
}
export function fromQuery(query: RuleGroupType): Condition[] {
  const number=(value:unknown)=>typeof value === "string" && !value.trim() ? Number.NaN : Number(value);
  const conditions=query.rules.map(row=> {
    if ("rules" in row) return fromQuery(row)[0];
    const rule=row as RuleType;
    switch (rule.field) {
      case "extension": return {type:"extension",exts:Array.isArray(rule.value)?rule.value:String(rule.value??"").split(/[,，\s]+/).filter(Boolean)} as Condition;
      case "name": return {type:"name_match",operator:({contains:"contains",doesNotContain:"not_contains","=":"equals",beginsWith:"starts_with",endsWith:"ends_with"} as const)[rule.operator as "contains"]??"contains",keyword:String(rule.value??"")} as Condition;
      case "size": {
        const values=Array.isArray(rule.value)?rule.value:String(rule.value??"").split(",");
        if (rule.operator === "between") return {type:"size_between",min:Math.round(number(values[0])*1048576),max:Math.round(number(values[1])*1048576)} as Condition;
        return {type:rule.operator === "<" ? "size_less_than" : "size_greater_than",bytes:Math.round(Number(rule.value)*1048576)} as Condition;
      }
      case "created": case "modified": return {type:"date_match",field:rule.field,operator:rule.operator === "older" ? "older_than_days" : "within_days",days:Number(rule.value)} as Condition;
      default: throw new Error("未知条件字段");
    }
  });
  return [{type:"group",mode:query.combinator === "or" ? "any" : "all",negate:!!query.not,conditions}];
}
