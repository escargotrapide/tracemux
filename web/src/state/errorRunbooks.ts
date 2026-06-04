const ERROR_RUNBOOK_BASE_URL = "https://github.com/escargotrapide/tracemux/blob/main/docs/errors";

export const KNOWN_ERROR_RUNBOOK_IDS = [
  "E-1001",
  "E-1002",
  "E-1003",
  "E-1101",
  "E-1102",
  "E-1103",
  "E-1104",
  "E-1105",
  "E-1106",
  "E-1301",
  "E-1401",
  "E-1402",
  "E-2001",
  "E-2002",
  "E-2101",
  "E-2102",
  "E-2103",
  "E-4001",
  "E-4002",
] as const;

const KNOWN_ERROR_RUNBOOK_SET = new Set<string>(KNOWN_ERROR_RUNBOOK_IDS);

export function errorRunbookPath(errorId: string | undefined): string | undefined {
  if (!errorId || !KNOWN_ERROR_RUNBOOK_SET.has(errorId)) return undefined;
  return `docs/errors/${errorId}.md`;
}

export function errorRunbookUrl(errorId: string | undefined): string | undefined {
  if (!errorRunbookPath(errorId)) return undefined;
  return `${ERROR_RUNBOOK_BASE_URL}/${errorId}.md`;
}

// Short, inline remedy hints for error ids that do not have a public runbook
// (legacy UI ids, protocol/transport ids, or otherwise unmapped ids). Every
// visible error therefore offers either a runbook link or an inline remedy.
const INLINE_REMEDY_KEYS: Record<string, string> = {
  "E-UI-0001": "errors.inline.E-UI-0001",
  "E-UI-0002": "errors.inline.E-UI-0002",
  "E-UI-ANNOTATION-SYNC": "errors.inline.E-UI-ANNOTATION-SYNC",
};

/**
 * Returns an i18n key for a short inline remedy hint for the given error id, or
 * `undefined` when the id already has a public runbook link. Unknown ids fall
 * back to a generic remedy so no visible error is left without guidance.
 */
export function errorInlineRemedyKey(errorId: string | undefined): string | undefined {
  if (!errorId) return undefined;
  if (KNOWN_ERROR_RUNBOOK_SET.has(errorId)) return undefined;
  return INLINE_REMEDY_KEYS[errorId] ?? "errors.inline.generic";
}
