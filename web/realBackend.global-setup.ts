import { startRealBackend } from "./tests/e2e/realBackend.harness";

export default async function globalSetup(): Promise<void> {
  await startRealBackend();
}
