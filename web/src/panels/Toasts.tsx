// Toast notifications (NFR-UX-001).
//
// REQ: FR-UI-009

import { For } from "solid-js";
import { toastsStore, dismissToast } from "~/state";

export function Toasts() {
  return (
    <div class="wl-toasts" data-testid="toasts">
      <For each={toastsStore}>
        {(t) => (
          <div class={`wl-toast wl-toast-${t.level}`} data-toast-id={t.id}>
            <span>{t.message}</span>
            {t.errorId ? (
              <span style={{ color: "var(--wl-fg-muted)" }}>
                {" "}
                ({t.errorId})
              </span>
            ) : null}
            <button
              type="button"
              class="wl-toast-close"
              onClick={() => dismissToast(t.id)}
              aria-label="dismiss"
            >
              Å~
            </button>
          </div>
        )}
      </For>
    </div>
  );
}
