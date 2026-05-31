export type StorageLike = Pick<Storage, "getItem" | "setItem">;

export function browserStorage(): StorageLike | undefined {
  if (typeof window === "undefined") return undefined;
  try {
    return window.localStorage;
  } catch {
    return undefined;
  }
}

export function safeGetItem(
  key: string,
  storage: StorageLike | undefined = browserStorage(),
): string | null {
  try {
    return storage?.getItem(key) ?? null;
  } catch {
    return null;
  }
}

export function safeSetItem(
  key: string,
  value: string,
  storage: StorageLike | undefined = browserStorage(),
): boolean {
  try {
    storage?.setItem(key, value);
    return storage !== undefined;
  } catch {
    return false;
  }
}