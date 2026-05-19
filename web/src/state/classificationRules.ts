import { createStore } from "solid-js/store";

export const CLASSIFICATION_RULES_STORAGE_KEY = "wanlogger.classificationRules.v1";
export const MAX_CLASSIFICATION_RULES = 200;
export const MAX_CLASSIFICATION_TEXT_LENGTH = 160;

export interface ClassificationRule {
  id: string;
  contains: string;
  tag: string;
  caseSensitive: boolean;
  enabled: boolean;
  updatedAt: number;
}

export type ClassificationRules = Record<string, ClassificationRule>;

export interface WireClassificationRule {
  contains: string;
  tag: string;
  case_sensitive?: boolean;
}

type StorageLike = Pick<Storage, "getItem" | "setItem">;

function defaultStorage(): StorageLike | undefined {
  if (typeof window === "undefined") return undefined;
  return window.localStorage;
}

function slugPart(value: string): string {
  return value
    .trim()
    .toLowerCase()
    .replace(/[^a-z0-9_-]+/g, "-")
    .replace(/^-+|-+$/g, "")
    .slice(0, 48);
}

function fallbackRuleId(contains: string, tag: string): string {
  const containsPart = slugPart(contains) || "contains";
  const tagPart = slugPart(tag) || "tag";
  return `${containsPart}-${tagPart}`;
}

function normalizeText(value: unknown): string {
  return typeof value === "string"
    ? value.trim().slice(0, MAX_CLASSIFICATION_TEXT_LENGTH)
    : "";
}

function normalizeRule(value: unknown, fallbackId: string): ClassificationRule | null {
  if (!value || typeof value !== "object") return null;
  const input = value as Partial<ClassificationRule>;
  const contains = normalizeText(input.contains);
  const tag = normalizeText(input.tag);
  if (!contains || !tag) return null;
  const id = normalizeText(input.id) || fallbackId || fallbackRuleId(contains, tag);
  const updatedAt = typeof input.updatedAt === "number" && Number.isFinite(input.updatedAt)
    ? Math.max(0, Math.trunc(input.updatedAt))
    : 0;
  return {
    id,
    contains,
    tag,
    caseSensitive: input.caseSensitive === true,
    enabled: input.enabled !== false,
    updatedAt,
  };
}

export function normalizeClassificationRules(value: unknown): ClassificationRules {
  if (!value || typeof value !== "object") return {};
  const out: ClassificationRules = {};
  for (const [id, raw] of Object.entries(value as Record<string, unknown>)) {
    if (Object.keys(out).length >= MAX_CLASSIFICATION_RULES) break;
    const rule = normalizeRule(raw, id);
    if (rule) out[rule.id] = rule;
  }
  return out;
}

export function loadClassificationRules(storage = defaultStorage()): ClassificationRules {
  const raw = storage?.getItem(CLASSIFICATION_RULES_STORAGE_KEY) ?? null;
  if (!raw) return {};
  try {
    return normalizeClassificationRules(JSON.parse(raw) as unknown);
  } catch {
    return {};
  }
}

export function saveClassificationRules(
  rules: ClassificationRules,
  storage = defaultStorage(),
): ClassificationRules {
  const normalized = normalizeClassificationRules(rules);
  storage?.setItem(CLASSIFICATION_RULES_STORAGE_KEY, JSON.stringify(normalized));
  return normalized;
}

const [classificationRulesStore, setClassificationRulesStore] = createStore<ClassificationRules>(
  loadClassificationRules(),
);

export const classificationRules = classificationRulesStore;

export function upsertClassificationRule(
  patch: Partial<ClassificationRule>,
  storage = defaultStorage(),
  now = Date.now(),
): ClassificationRule {
  const id = normalizeText(patch.id) || fallbackRuleId(patch.contains ?? "", patch.tag ?? "");
  const rule = normalizeRule({ ...patch, id, updatedAt: now }, id);
  if (!rule) {
    throw new Error("classification rule requires contains and tag");
  }
  setClassificationRulesStore(rule.id, rule);
  saveClassificationRules({ ...classificationRulesStore, [rule.id]: rule }, storage);
  return rule;
}

export function deleteClassificationRule(
  id: string,
  storage = defaultStorage(),
): void {
  setClassificationRulesStore(id, undefined as unknown as ClassificationRule);
  const next = { ...classificationRulesStore };
  delete next[id];
  saveClassificationRules(next, storage);
}

export function orderedClassificationRules(
  rules: ClassificationRules = classificationRulesStore,
): ClassificationRule[] {
  return Object.values(rules).sort((a, b) => a.updatedAt - b.updatedAt || a.id.localeCompare(b.id));
}

export function enabledClassificationRules(
  rules: ClassificationRules = classificationRulesStore,
): ClassificationRule[] {
  return orderedClassificationRules(rules).filter((rule) => rule.enabled);
}

export function classifyText(
  text: string,
  rules: ClassificationRule[] = enabledClassificationRules(),
): string[] {
  const out: string[] = [];
  const seen = new Set<string>();
  for (const rule of rules) {
    const haystack = rule.caseSensitive ? text : text.toLowerCase();
    const needle = rule.caseSensitive ? rule.contains : rule.contains.toLowerCase();
    if (!needle || !haystack.includes(needle) || seen.has(rule.tag)) continue;
    seen.add(rule.tag);
    out.push(rule.tag);
  }
  return out;
}

export function wireClassificationRules(
  rules: ClassificationRule[] = enabledClassificationRules(),
): WireClassificationRule[] {
  return rules.map((rule) => ({
    contains: rule.contains,
    tag: rule.tag,
    ...(rule.caseSensitive ? { case_sensitive: true } : {}),
  }));
}
