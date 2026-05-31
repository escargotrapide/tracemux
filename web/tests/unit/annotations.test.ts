import { describe, expect, it } from "vitest";
import {
  annotationIdForTarget,
  deleteServerAnnotation,
  listServerAnnotations,
  logTypeAnnotationTarget,
  normalizeServerAnnotations,
  putServerAnnotation,
  serverAnnotationsUrl,
  sessionAnnotationTarget,
  type FetchLike,
} from "../../src/adapters/annotations";

const UUID_RE = /^[0-9a-f]{8}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{12}$/i;

function jsonResponse(value: unknown, status = 200): Response {
  return new Response(JSON.stringify(value), {
    status,
    headers: { "Content-Type": "application/json" },
  });
}

describe("annotation HTTP adapter", () => {
  it("derives stable annotation ids from targets", () => {
    const sessionId = annotationIdForTarget(sessionAnnotationTarget("ABCDEFAB-1234-5678-9ABC-DEF012345678"));
    const sameSessionId = annotationIdForTarget(sessionAnnotationTarget("abcdefab-1234-5678-9abc-def012345678"));
    const logTypeId = annotationIdForTarget(logTypeAnnotationTarget("fault"));

    expect(sessionId).toMatch(UUID_RE);
    expect(sessionId).toBe(sameSessionId);
    expect(logTypeId).toMatch(UUID_RE);
    expect(logTypeId).not.toBe(sessionId);
  });

  it("normalizes server annotation arrays", () => {
    const id = annotationIdForTarget(logTypeAnnotationTarget("fault"));
    expect(
      normalizeServerAnnotations([
        {
          id,
          target: { kind: "log_type", key: " fault " },
          text: "watch motor",
          updated_at: "2026-05-20T00:00:00Z",
          updated_by: " alice ",
        },
        { id: "bad", target: null },
      ]),
    ).toEqual([
      {
        id,
        target: { kind: "log_type", key: "fault" },
        text: "watch motor",
        updated_at: "2026-05-20T00:00:00Z",
        updated_by: "alice",
        deleted: false,
      },
    ]);
  });

  it("lists annotations with an optional sid query", async () => {
    const calls: Array<[RequestInfo | URL, RequestInit | undefined]> = [];
    const id = annotationIdForTarget(sessionAnnotationTarget("sid1"));
    const fetchImpl: FetchLike = async (input, init) => {
      calls.push([input, init]);
      return jsonResponse([
        {
          id,
          target: { kind: "session", sid: "sid1" },
          text: "memo",
          updated_at: "2026-05-20T00:00:00Z",
          deleted: false,
        },
      ]);
    };

    const annotations = await listServerAnnotations({ sid: "sid1" }, fetchImpl);

    expect(serverAnnotationsUrl({ sid: "sid1" })).toContain("/api/annotations?sid=sid1");
    expect(String(calls[0]?.[0])).toContain("/api/annotations?sid=sid1");
    expect(annotations).toHaveLength(1);
    expect(annotations[0]?.text).toBe("memo");
  });

  it("saves and deletes annotations over HTTP", async () => {
    const calls: Array<[RequestInfo | URL, RequestInit | undefined]> = [];
    const target = logTypeAnnotationTarget("fault");
    const id = annotationIdForTarget(target);
    const fetchImpl: FetchLike = async (input, init) => {
      calls.push([input, init]);
      if (init?.method === "DELETE") return new Response(null, { status: 404 });
      return jsonResponse({
        id,
        target,
        text: "note",
        updated_at: "2026-05-20T00:00:00Z",
        deleted: false,
      });
    };

    const saved = await putServerAnnotation(id, { target, text: "note" }, fetchImpl);
    await deleteServerAnnotation(id, fetchImpl);

    expect(saved.id).toBe(id);
    expect(calls[0]?.[1]?.method).toBe("PUT");
    expect(calls[0]?.[1]?.body).toBe(JSON.stringify({ target, text: "note" }));
    expect(calls[1]?.[1]?.method).toBe("DELETE");
  });
});
