# Account Editing and Interactive Login Implementation Plan

> **For agentic workers:** Execute independent backend account editing and credential persistence tasks in parallel; integrate login interaction and UI sequentially. All builds/tests run only in GitHub Actions.

**Goal:** Optional account login metadata, editable accounts, interactive password/SMS/QR platform login, encrypted credential restore and accurate login status.

**Architecture:** Reuse account-isolated BrowserContext. Add a login command module for CDP input and non-navigating login confirmation. Separate session-open state from authentication. Extend encrypted cookie payload with versioned web storage and legacy compatibility.

**Tech Stack:** Tauri 2, chromiumoxide 0.7, SQLite, Vue 3, TypeScript.

## Tasks
- [ ] Account metadata: UpdateAccount DTO; DB update preserving blank password; explicit clear; optional username; fix NULL credential reads. Add account_edit command and sidebar editing. Proxy edits close old session.
- [ ] Credential snapshot: parse legacy array and v1 envelope; restore cookies before navigation; restore web storage only matching trusted origin; propagate persistence failures. Add pure parse tests.
- [ ] Login commands: viewport click, insert text, keyboard keys and wheel; bounded inputs; no secrets in errors. Return screenshot viewport size for coordinate mapping. Session interactive flag prevents background navigation.
- [ ] Login confirmation: non-navigating evidence check, unknown on insufficient evidence, only authenticated session saved and marked online. No initial-login SMS alerts. Distinguish manual explicit confirmation for unsupported custom evidence.
- [ ] Workspace: remount per account ID; isolate input/timers; password/SMS/QR instructions, viewport clickable image, input send and key controls, QR refresh, finish-and-save and status. Refresh sidebar after auth updates.
- [ ] CI: add unit test filters for credentials and account tests, build three platforms, inspect results, fix failures, bump date version and publish tagged release. Do not run local compiler or tests.

## Acceptance
Creating account only needs platform/name; edits keep ID/tasks; blank password preserves and clear deletes; screenshots map using natural dimensions; codes never stored; QR refresh does not navigate; unknown is not online; old payload restores; failed storage save is visible; proxy change clears current session; account tabs do not reuse state. Real SMS/QR success must be validated with user-owned account and phone, not inferred from CI.
