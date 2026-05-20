import {
  annotationIdForTarget,
  deleteServerAnnotation,
  listServerAnnotations,
  logTypeAnnotationTarget,
  putServerAnnotation,
  sessionAnnotationTarget,
  type FetchLike,
  type ServerAnnotation,
} from "~/adapters/annotations";
import { logTypeNotes, updateLogTypeNote, type LogTypeNote } from "~/state/logTypeNotes";
import { sourceNotes, updateSourceNote, type SourceNote } from "~/state/sourceNotes";

type StorageLike = Pick<Storage, "getItem" | "setItem">;

export interface ApplyServerAnnotationsOptions {
  storage?: StorageLike;
  now?: number;
  includeScopedLogTypes?: boolean;
}

function annotationTimeMs(annotation: ServerAnnotation, fallback: number): number {
  const ms = Date.parse(annotation.updated_at);
  return Number.isFinite(ms) ? ms : fallback;
}

function shouldApply(localUpdatedAt: number | undefined, serverUpdatedAt: number): boolean {
  return localUpdatedAt === undefined || serverUpdatedAt >= localUpdatedAt;
}

export function applyServerAnnotations(
  annotations: ServerAnnotation[],
  options: ApplyServerAnnotationsOptions = {},
): void {
  const fallbackNow = options.now ?? Date.now();
  for (const annotation of annotations) {
    if (annotation.deleted) continue;
    const updatedAt = annotationTimeMs(annotation, fallbackNow);
    if (annotation.target.kind === "session") {
      const sid = annotation.target.sid?.trim();
      if (!sid) continue;
      const local = sourceNotes[sid];
      if (shouldApply(local?.updatedAt, updatedAt)) {
        updateSourceNote(sid, annotation.text, options.storage, updatedAt);
      }
      continue;
    }

    const key = annotation.target.key?.trim();
    if (!key) continue;
    if (annotation.target.sid && !options.includeScopedLogTypes) continue;
    const local = logTypeNotes[key];
    if (shouldApply(local?.updatedAt, updatedAt)) {
      updateLogTypeNote(key, annotation.text, options.storage, updatedAt);
    }
  }
}

export async function loadAndApplySourceAnnotations(
  sid: string,
  fetchImpl: FetchLike = fetch,
  options: ApplyServerAnnotationsOptions = {},
): Promise<void> {
  const annotations = await listServerAnnotations({ sid }, fetchImpl);
  applyServerAnnotations(annotations, { ...options, includeScopedLogTypes: true });
}

export async function loadAndApplyLogTypeAnnotations(
  fetchImpl: FetchLike = fetch,
  options: ApplyServerAnnotationsOptions = {},
): Promise<void> {
  const annotations = await listServerAnnotations({}, fetchImpl);
  applyServerAnnotations(annotations, options);
}

export async function syncSourceNoteToServer(
  note: SourceNote,
  fetchImpl: FetchLike = fetch,
): Promise<void> {
  const target = sessionAnnotationTarget(note.sid);
  const id = annotationIdForTarget(target);
  if (note.text.length === 0) {
    await deleteServerAnnotation(id, fetchImpl);
    return;
  }
  await putServerAnnotation(id, { target, text: note.text }, fetchImpl);
}

export async function syncLogTypeNoteToServer(
  note: LogTypeNote,
  fetchImpl: FetchLike = fetch,
): Promise<void> {
  const target = logTypeAnnotationTarget(note.key);
  const id = annotationIdForTarget(target);
  if (note.text.length === 0) {
    await deleteServerAnnotation(id, fetchImpl);
    return;
  }
  await putServerAnnotation(id, { target, text: note.text }, fetchImpl);
}
