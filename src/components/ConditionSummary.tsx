import { Fragment } from "react";
import { describeCondition, type Condition, type Rule } from "../lib/types";

export default function ConditionSummary({conditions,target,mode="all"}:{conditions:Condition[];target:Rule["target_kind"];mode?:"all"|"any"}) {
  return <>{conditions.map((condition,index)=><Fragment key={index}>
    {index>0 && (mode === "all" ? " 且 " : " 或 ")}
    {condition.type === "group" ? <>{condition.negate && "排除"}（<ConditionSummary conditions={condition.conditions} target={target} mode={condition.mode}/>）</> :
      condition.type === "extension" ? <>扩展名是 {condition.exts.map((ext,i)=><span className="extension" key={i}>.{ext.replace(/^\./,"")}</span>)}</> : describeCondition(condition,target)}
  </Fragment>)}</>;
}
