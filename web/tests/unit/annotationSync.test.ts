import { describe, expect, it } from "vitest";
import { annotationIdForTarget, sessionAnnotationTarget, type FetchLike } from "../../src/adapters/annotations";
import {
  applyServerAnnotations,
  syncLogTypeNoteToServer,
  syncSourceNoteToServer,
} from "../../src/state/annotationSync";
import { loadLogTypeNotes, updateLogTypeNote } from "../../src/state/logTypeNotes";
import { loadSourceNotes, updateSourceNote } from "../../src/state/sourceNotes";

class FakeStorage implements Pick<Storage, "getItem" | "setItem"> {
  private readonly data = new Map<string, string>();

  getItem(key: string): string | null {
    return this.data.get(key) ?? null;
  }

  setItem(key: string, value: string): void {
    this.data.set(key, value);
  }
}

function okAnnotationResponse(id: string, target: unknown, text: string): Response {
  return new Response(
    JSON.stringify({
      id,
      target,
      text,
      updated_at: "2026-05-20T00:00:00Z",
      deleted: false,
    }),
    { status: 200, headers: { "Content-Type": "application/json" } },
  );
}

describe("annotation sync state helpers", () => {
  it("applies newer server session and log-type notes to local fallback storage", () => {
    const storage = new FakeStorage();
    const sid = "11111111-1111-4111-8111-111111111111";
    const key = "sync-test-fault";
    const serverMs = Date.parse("2026-05-20T00:00:00Z");

    applyServerAnnotations(
      [
        {
          id: annotationIdForTarget(sessionAnnotationTarget(sid)),
          target: { kind: "session", sid },
          text: "server source note",
          updated_at: "2026-05-20T00:00:00Z",
          deleted: false,
        },
        {
          id: annotationIdForTarget({ kind: "log_type", key }),
          target: { kind: "log_type", key },
          text: "server type note",
          updated_at: "2026-05-20T00:00:00Z",
          deleted: false,
        },
      ],
      { storage },
    );

    expect(loadSourceNotes(storage)[sid]?.text).toBe("server source note");
    expect(loadSourceNotes(storage)[sid]?.updatedAt).toBe(serverMs);
    expect(loadLogTypeNotes(storage)[key]?.text).toBe("server type note");

    updateSourceNote(sid, "newer local source note", storage, serverMs + 1);
    updateLogTypeNote(key, "newer local type note", storage, serverMs + 1);

    applyServerAnnotations(
      [
        {
          id: annotationIdForTarget(sessionAnnotationTarget(sid)),
          target: { kind: "session", sid },
          text: "older server source note",
          updated_at: "2026-05-20T00:00:00Z",
          deleted: false,
        },
        {
          id: annotationIdForTarget({ kind: "log_type", key }),
          target: { kind: "log_type", key },
          text: "older server type note",
          updated_at: "2026-05-20T00:00:00Z",
          deleted: false,
        },
      ],
      { storage },
    );

    expect(loadSourceNotes(storage)[sid]?.text).toBe("newer local source note");
    expect(loadLogTypeNotes(storage)[key]?.text).toBe("newer local type note");
  });

  it("applies newer server deletes without erasing newer local notes", () => {
    const storage = new FakeStorage();
    const sid = "55555555-5555-4555-8555-555555555555";
    const key = "deleted-fault";
    const serverMs = Date.parse("2026-05-20T00:00:00Z");
    updateSourceNote(sid, "local source note", storage, serverMs - 1);
    updateLogTypeNote(key, "local type note", storage, serverMs - 1);

    applyServerAnnotations(
      [
        {
          id: annotationIdForTarget(sessionAnnotationTarget(sid)),
          target: { kind: "session", sid },
          text: "",
          updated_at: "2026-05-20T00:00:00Z",
          deleted: true,
        },
        {
          id: annotationIdForTarget({ kind: "log_type", key }),
          target: { kind: "log_type", key },
          text: "",
          updated_at: "2026-05-20T00:00:00Z",
          deleted: true,
        },
      ],
      { storage },
    );

    expect(loadSourceNotes(storage)[sid]).toBeUndefined();
    expect(loadLogTypeNotes(storage)[key]).toBeUndefined();

    updateSourceNote(sid, "newer local source note", storage, serverMs + 1);
    updateLogTypeNote(key, "newer local type note", storage, serverMs + 1);
    applyServerAnnotations(
      [
        {
          id: annotationIdForTarget(sessionAnnotationTarget(sid)),
          target: { kind: "session", sid },
          text: "",
          updated_at: "2026-05-20T00:00:00Z",
          deleted: true,
        },
        {
          id: annotationIdForTarget({ kind: "log_type", key }),
          target: { kind: "log_type", key },
          text: "",
          updated_at: "2026-05-20T00:00:00Z",
          deleted: true,
        },
      ],
      { storage },
    );

    expect(loadSourceNotes(storage)[sid]?.text).toBe("newer local source note");
    expect(loadLogTypeNotes(storage)[key]?.text).toBe("newer local type note");
  });

  it("syncs source notes with deterministic annotation ids", async () => {
    const calls: Array<[RequestInfo | URL, RequestInit | undefined]> = [];
    const sid = "22222222-2222-4222-8222-222222222222";
    const target = sessionAnnotationTarget(sid);
    const id = annotationIdForTarget(target);
    const fetchImpl: FetchLike = async (input, init) => {
      calls.push([input, init]);
      return okAnnotationResponse(id, target, "memo");
    };

    await syncSourceNoteToServer({ sid, text: "memo", updatedAt: 1 }, fetchImpl);

    expect(String(calls[0]?.[0])).toContain(`/api/annotations/${encodeURIComponent(id)}`);
    expect(calls[0]?.[1]?.method).toBe("PUT");
    expect(calls[0]?.[1]?.body).toBe(JSON.stringify({ target, text: "memo" }));
  });

  it("deletes empty source notes and syncs log-type notes", async () => {
    const calls: Array<[RequestInfo | URL, RequestInit | undefined]> = [];
    const fetchImpl: FetchLike = async (input, init) => {
      calls.push([input, init]);
      if (init?.method === "DELETE") return new Response(null, { status: 204 });
      return okAnnotationResponse("33333333-3333-5333-8333-333333333333", { kind: "log_type", key: "fault" }, "watch");
    };

    await syncSourceNoteToServer({ sid: "33333333-3333-4333-8333-333333333333", text: "", updatedAt: 1 }, fetchImpl);
    await syncLogTypeNoteToServer({ key: "fault", text: "watch", updatedAt: 2 }, fetchImpl);

    expect(calls[0]?.[1]?.method).toBe("DELETE");
    expect(calls[1]?.[1]?.method).toBe("PUT");
    expect(calls[1]?.[1]?.body).toContain("fault");
  });
});
