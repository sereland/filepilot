import { invoke } from "@tauri-apps/api/core";
import type { OpRecord, PlanItem, Rule } from "./types";

export const api = {
  getRules: (): Promise<Rule[]> => invoke("get_rules"),
  saveRule: (rule: Rule): Promise<Rule> => invoke("save_rule", { rule }),
  deleteRule: (id: string): Promise<boolean> => invoke("delete_rule", { id }),
  setRuleEnabled: (id: string, enabled: boolean): Promise<boolean> =>
    invoke("set_rule_enabled", { id, enabled }),
  previewRule: (id: string): Promise<PlanItem[]> =>
    invoke("preview_rule", { id }),
  applyRuleNow: (id: string, selectedSources: string[]): Promise<OpRecord[]> =>
    invoke("apply_rule_now", { id, selectedSources }),
  getRuleLastRuns: (): Promise<Record<string, string>> => invoke("get_rule_last_runs"),
  getOplog: (limit: number): Promise<OpRecord[]> =>
    invoke("get_oplog", { limit }),
  undoLast: (): Promise<number> => invoke("undo_last"),
  startMonitoring: (): Promise<boolean> => invoke("start_monitoring"),
  stopMonitoring: (): Promise<void> => invoke("stop_monitoring"),
  isMonitoring: (): Promise<boolean> => invoke("is_monitoring"),
  setAutostart: (enabled: boolean): Promise<boolean> =>
    invoke("set_autostart", { enabled }),
  isAutostartEnabled: (): Promise<boolean> => invoke("is_autostart_enabled"),
  pickFolder: (): Promise<string | null> => invoke("pick_folder"),
};
