import { describe, expect, it } from "vitest";
import { setLocale, t } from "../../src/i18n";

describe("i18n", () => {
  it("has pcap publish mode labels and hints", () => {
    setLocale("en");
    expect(t("sources.pcap.publish_mode.stats-only")).toContain("metrics only");
    expect(t("sources.pcap.publish_hint.stats-only")).toContain("Packet list stays empty");
    expect(t("sources.export.shared_settings")).toContain("single-source and bulk");
    expect(t("sources.serial.open_results")).toContain("Serial open results");
    expect(t("sources.serial.open_result.start_sent")).toContain("start request sent");
    expect(t("sources.serial.detect_progress")).toContain("Scanning COM ports");
    expect(t("sources.export_all.progress")).toContain("ZIP download");
    expect(t("sources.export_all.cancel")).toContain("Cancel export");
    expect(t("sources.detail.recent_errors")).toContain("Recent errors");

    setLocale("ja");
    expect(t("sources.pcap.publish_mode.full")).toContain("全packet");
    expect(t("sources.pcap.publish_hint.full")).toContain("全packetをUIへ流します");
    expect(t("sources.export.shared_settings")).toContain("単体エクスポートと一括ZIP");
    expect(t("sources.serial.open_results")).toContain("シリアル開始結果");
    expect(t("sources.serial.open_result.failed")).toContain("失敗");
    expect(t("sources.serial.detect_progress")).toContain("COMポートを走査中");
    expect(t("sources.export.progress")).toContain("ダウンロード準備中");
    expect(t("sources.export_all.cancelled")).toContain("キャンセルしました");
    expect(t("sources.detail.no_recent_errors")).toContain("最近のソースエラーはありません");

    setLocale("en");
  });
});
