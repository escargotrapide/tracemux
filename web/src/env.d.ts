/* eslint-disable @typescript-eslint/triple-slash-reference */
/// <reference types="vite/client" />

declare const __TRACEMUX_WIRE__: string;

interface ImportMetaEnv {
  readonly VITE_TRACEMUX_URL?: string;
  readonly VITE_TRACEMUX_TOKEN?: string;
}

interface ImportMeta {
  readonly env: ImportMetaEnv;
}
