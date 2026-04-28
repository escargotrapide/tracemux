/* eslint-disable @typescript-eslint/triple-slash-reference */
/// <reference types="vite/client" />

declare const __WANLOGGER_WIRE__: string;

interface ImportMetaEnv {
  readonly VITE_WANLOGGER_URL?: string;
  readonly VITE_WANLOGGER_TOKEN?: string;
}

interface ImportMeta {
  readonly env: ImportMetaEnv;
}
