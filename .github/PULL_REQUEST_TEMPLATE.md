<!--
Thank you for your contribution. Fill in every section. AI-authored PRs
must pass `just ai-verify` before requesting human review.
-->

## 受入条件 (Acceptance criteria)

<!-- Bulleted, testable. Reference requirement ids (FR-/NFR-). -->
- [ ] FR-…

## 影響範囲 (Scope of change)

<!-- Layers / files / users affected. -->

## wire / log / cli 互換性影響 (Compatibility)

- [ ] No change to `docs/protocols/wire-protocol.md`
- [ ] No change to `docs/protocols/log-format.md`
- [ ] No change to `docs/protocols/cli-output/v1/**`
- [ ] If any of the above is checked, an ADR is included and the
  matching version was bumped, plus compat fixtures updated.

## RTM 更新 (Requirements Traceability Matrix)

- [ ] `docs/requirements.md` updated (new / amended FR / NFR)
- [ ] Tests reference the requirement id (`// REQ: FR-…`)
- [ ] `just rtm` regenerated `docs/rtm.md`

## Verification

- [ ] `just ai-verify` is green (`target/ai-verify.json` attached or
  posted in the PR)
- [ ] If a critical path was touched, the
  `human-review-required` label is present and not removed.

## Notes for reviewers
