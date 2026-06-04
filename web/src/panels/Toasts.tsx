// Toast notifications (NFR-UX-001).
//
// REQ: FR-UI-009

import { For, Show, createMemo, createSignal } from "solid-js";
import { t } from "~/i18n";
import { errorInlineRemedyKey, errorRunbookPath, errorRunbookUrl } from "~/state/errorRunbooks";
import {
  clearNotificationHistory,
  dismissAllToasts,
  dismissToast,
  notificationHistoryStore,
  toastsStore,
} from "~/state";

function formatNotificationTime(ts: number): string {
  return new Date(ts).toLocaleTimeString([], {
    hour: "2-digit",
    minute: "2-digit",
    second: "2-digit",
  });
}

function ErrorIdWithRunbook(props: { errorId: string }) {
  const path = () => errorRunbookPath(props.errorId);
  const url = () => errorRunbookUrl(props.errorId);
  const remedyKey = () => errorInlineRemedyKey(props.errorId);

  return (
    <>
      <span class="wl-error-id"> ({props.errorId})</span>
      <Show when={url()}>
        {(href) => (
          <a
            class="wl-error-runbook"
            href={href()}
            target="_blank"
            rel="noreferrer"
            title={path()}
          >
            {t("notifications.runbook")}
          </a>
        )}
      </Show>
      <Show when={remedyKey()}>
        {(key) => <span class="wl-error-remedy"> {t(key())}</span>}
      </Show>
    </>
  );
}

export function Toasts() {
  const [historyOpen, setHistoryOpen] = createSignal(false);
  const historyItems = createMemo(() => [...notificationHistoryStore].reverse());
  const notificationCount = () => notificationHistoryStore.reduce(
    (sum, item) => sum + item.count,
    0,
  );

  return (
    <>
      <div class="wl-notification-entry">
        <button
          type="button"
          class="wl-notification-button"
          data-testid="notification-button"
          aria-haspopup="dialog"
          aria-expanded={historyOpen()}
          title={t("notifications.open")}
          onClick={() => setHistoryOpen((open) => !open)}
        >
          <span>{t("notifications.title")}</span>
          <span class="wl-notification-count">{notificationCount()}</span>
        </button>
        <Show when={historyOpen()}>
          <section
            class="wl-notification-center"
            data-testid="notification-center"
            role="dialog"
            aria-label={t("notifications.title")}
          >
            <header class="wl-notification-center-header">
              <strong>{t("notifications.title")}</strong>
              <span class="wl-notification-center-actions">
                <button
                  type="button"
                  onClick={dismissAllToasts}
                  disabled={toastsStore.length === 0}
                >
                  {t("notifications.dismiss_all")}
                </button>
                <button
                  type="button"
                  onClick={clearNotificationHistory}
                  disabled={notificationHistoryStore.length === 0}
                >
                  {t("notifications.clear")}
                </button>
              </span>
            </header>
            <Show
              when={historyItems().length > 0}
              fallback={<p class="wl-notification-empty">{t("notifications.empty")}</p>}
            >
              <ol class="wl-notification-list">
                <For each={historyItems()}>
                  {(item) => (
                    <li class={`wl-notification-item wl-notification-${item.level}`}>
                      <span class="wl-notification-meta">
                        <span>{item.level}</span>
                        <time datetime={new Date(item.lastTs).toISOString()}>
                          {formatNotificationTime(item.lastTs)}
                        </time>
                        <Show when={item.count > 1}>
                          <span>x{item.count}</span>
                        </Show>
                      </span>
                      <span class="wl-notification-message">
                        {item.message}
                        <Show when={item.errorId}>
                          {(errorId) => <ErrorIdWithRunbook errorId={errorId()} />}
                        </Show>
                      </span>
                    </li>
                  )}
                </For>
              </ol>
            </Show>
          </section>
        </Show>
      </div>
      <div class="wl-toasts" data-testid="toasts" aria-live="polite">
        <For each={toastsStore}>
          {(item) => (
            <div class={`wl-toast wl-toast-${item.level}`} data-toast-id={item.id}>
              <span class="wl-toast-message">
                {item.message}
                <Show when={item.errorId}>
                  {(errorId) => <ErrorIdWithRunbook errorId={errorId()} />}
                </Show>
                <Show when={item.count > 1}>
                  <span class="wl-toast-count"> x{item.count}</span>
                </Show>
              </span>
              <button
                type="button"
                class="wl-toast-close"
                onClick={() => dismissToast(item.id)}
                aria-label={t("notifications.dismiss")}
              >
                &times;
              </button>
            </div>
          )}
        </For>
      </div>
    </>
  );
}
