import { stopRealBackend } from "./tests/e2e/realBackend.harness";

export default function globalTeardown(): void {
  stopRealBackend();
}
