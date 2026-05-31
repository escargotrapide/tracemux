<!--
Thank you for your contribution. Fill in every section. AI-authored PRs
must pass `just ai-verify` before requesting human review.
-->

## 受入条件 (Acceptance criteria)

<!-- Bulleted, testable. Reference requirement ids (FR-/NFR-). -->
- [ ] FR-...

## 影響範囲 (Scope of change)

<!-- Layers / files / users affected. -->
- [ ] Server / WSS / ingest
- [ ] Core pipeline traits or implementations
- [ ] CLI / schemas / compat fixtures
- [ ] Web UI / Tauri shell
- [ ] CI / scripts / release gates
- [ ] Docs only

## Critical path review

<!-- Check AGENTS.md section 5 before requesting review. AI agents must not self-merge critical-path PRs. -->
- [ ] I checked `AGENTS.md` critical paths.
- [ ] This PR touches a critical path and needs `human-review-required`.
- [ ] This PR does not touch a critical path.

## wire / log / cli 互換性影響 (Compatibility)

- [ ] No change to `docs/protocols/wire-protocol.md`
- [ ] No change to `docs/protocols/log-format.md`
- [ ] No change to `docs/protocols/cli-output/v1/**`
- [ ] No frozen v0.1 trait surface changed (`Source`, `Framer`, `Decoder`, `LogSink`, etc.)
- [ ] If any frozen surface changed, an ADR is included and the
  matching version was bumped, plus compat fixtures were updated.

## RTM 更新 (Requirements Traceability Matrix)

- [ ] `docs/requirements.md` updated (new / amended FR / NFR)
- [ ] Tests reference the requirement id (`// REQ: FR-...`)
- [ ] `just rtm` regenerated `docs/rtm.md`

## Verification

- [ ] `just ai-verify` is green (`target/ai-verify.json` attached or
  posted in the PR)
- [ ] Web changes: `pnpm --dir web test -- --run` and `pnpm --dir web build` are green
- [ ] Release-bound PRs: `scripts/release-gate.*` was run or blockers are documented
- [ ] If a critical path was touched, the
  `human-review-required` label is present and not removed.

## Notes for reviewers

<!-- Include known follow-ups, skipped checks, local-only issues, and critical-path rationale. -->
