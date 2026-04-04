# Changelog

## [0.5.1] - 2026-04-04

### 📋 Release Summary

This update improves search usability by ensuring consistent interface focus during navigation (f2d9bbc9). Additionally, system stability and performance have been enhanced by resolving issues related to interrupted connections and hanging requests (2f66801f, 595e5ebb).


### 🐛 Bug Fixes & Stability

- **search**: maintain modal focus during tab switch `f2d9bbc9`
- **acp**: prevent connection leaks on cancel `2f66801f`
- **mcp**: prevent hanging navigation requests `595e5ebb`

## [0.5.0] - 2026-04-04

### 📋 Release Summary

This update introduces advanced browser automation tools, instant navigation support, and a more proactive AI assistant featuring persistent prompt history (8129da4e, 0aece8d4, 6c97b7e8, 1dc8bb7c, 569bb103). User experience is further refined with new sidebar code-management tools and critical fixes for tab restoration, modal styling, and prompt stability (d215ff09, 0fbd3727, 4e05cdff, 4088d5dd).


### ✨ New Features & Enhancements

- **mcp**: stabilize navigation and interaction `8129da4e`
- **sidebar**: add code headers and copy button `d215ff09`
- **learning**: implement proactive background agent `1dc8bb7c`
- **ai**: add persistent prompt history to assistant `569bb103`
- **mcp**: expand browser automation tools `0aece8d4`
- **webview**: enable instant back-forward support `6c97b7e8`

### 🐛 Bug Fixes & Stability

- **sidebar**: use glass-solid modal background `0fbd3727`
- **tabs**: prevent URL clobbering on restore `4e05cdff`
- **acp**: prevent timeout during active prompts `4088d5dd`

## [0.4.0] - 2026-04-02

### 📋 Release Summary

This update introduces powerful AI editing capabilities including multi-modal image support, persistent prompt history, and a new inline editor with dedicated shortcuts (fd4ddc72, 96362396, 62294b7d, 5e29bbf5, d1ea76eb, 44722aa6). The user experience is further enhanced with interactive PDF and DOCX support, fuzzy search, and a comprehensive settings interface for better workspace customization (283f66fd, 68b5edab, 5f50040b, a678e789, 182f1f79, 327ccd81). General refinements to tool monitoring and UI positioning ensure a more stable and responsive experience (1fed407c, 0516a8d6, f9e748f4, 1898076b, 184bd1ea, 866d1595).


### 🚨 Breaking Changes

⚠️ **Important**: This release contains breaking changes that may require code updates.

- **acp**: add raw tool data and UI details `fd4ddc72`
- **acp**: add multi-modal image support `96362396`
- **agents**: improve monitoring and screenshots `62294b7d`

### ✨ New Features & Enhancements

- **shortcuts**: add AI Editor column to overlay `5e29bbf5`
- **ai**: implement persistent prompt history `d1ea76eb`
- **ui**: add custom window dragging via IPC `182f1f79`
- **settings**: add settings view and config UI `a678e789`
- **inline-edit**: add auto and manual modal hiding `f9e748f4`
- **editor**: improve inline edit UI and positioning `1898076b`
- **ai**: add inline text editing via Cmd+Shift+E `44722aa6`
- **zoom**: add page zoom controls and shortcuts `327ccd81`
- **sidebar**: add tool execution details modal `1fed407c`
- **sidebar**: show tool usage count in headers `0516a8d6`
- **pdf**: add interactive document rendering `283f66fd`
- **search**: add fuzzy search and markdown `5f50040b`
- **sidebar**: add PDF and DOCX attachment support `68b5edab`

### 🐛 Bug Fixes & Stability

- **editor**: trim whitespace from edit response `184bd1ea`
- **webview**: use browser window as footer parent `866d1595`

## [0.3.0] - 2026-04-01

### 📋 Release Summary

This release adds improved crash reporting with exit diagnostics, enhanced history autosave with atomic writes, and smarter overlay behavior including auto-hide when the app loses focus. Several bug fixes improve focus handling, browser tab stability, and prevent stale timers (938f9b42, 0b5ffcf5, 21a81d85).


### ✨ New Features & Enhancements

- **crash-report**: add exit trigger logging with backtrace `554a7ff8`
- **history**: add atomic writes and debounce saves `ead932cd`
- **overlay**: add auto-hide when app loses focus `904e499f`
- **overlay**: smarter autofill and selection tracking `5f3c6eeb`

### 🔧 Improvements & Optimizations

- **overlay**: extract dismiss closure and control cursor visibility `3504014c`

### 🐛 Bug Fixes & Stability

- **cold_open**: capture kAEGetURL before EventLoop::new() `954711d4`
- correct debounce comment from 5s to 60s `8d7ca7c0`
- **focus**: include overlay window in focus tracking logic `938f9b42`
- **overlay**: suppress hover selection until pointer movement `46e891d4`
- **browser**: store URL when opening tabs instead of relying on update_url `0b5ffcf5`
- **agents**: prevent stale timers and add browser timeouts `21a81d85`

## [0.2.0] - 2026-03-28

### 📋 Release Summary

This release brings significant improvements to privacy and performance. Enhanced tracker blocking, autoplay prevention, and WebKit content blocking give users better control over their browsing experience. Tab management has been improved with frozen snapshots and proactive hibernation for better resource usage (7aecc161, f2fa86f9, acba3a27, 4f12c157).


### ✨ New Features & Enhancements

- **acp**: add exponential backoff reconnection `7aecc161`
- **tabs**: add frozen tab snapshots and speculative preconnect `f2fa86f9`
- **blocklist**: expand tracker blocking and add autoplay prevention `acba3a27`
- add WebKit content blocking and proactive tab hibernation `4f12c157`

### 🔧 Improvements & Optimizations

- reformat code with rustfmt `4ea6a809`

### 🔄 Other Changes

1 maintenance, dependency, and tooling update not listed individually.

## [0.1.1] - 2026-03-26

### 📋 Release Summary

This release improves search accuracy by prioritizing exact domain matches and enhances macOS compatibility with consistent English locale handling (d858b39c, 3f245319). Additional stability improvements and streamlined result presentation deliver a more reliable browsing experience.


### ✨ New Features & Enhancements

- **search**: prioritize exact domain matches in fuzzy ranking `d858b39c`
- **macos**: force English locale for WKWebView Accept-Language header `3f245319`

### 🔧 Improvements & Optimizations

- **release**: add homebrew tap notification job `97e80a07`
- **overlay**: remove grouped sections and render flat by score `1fcea3cd`

### 🐛 Bug Fixes & Stability

- **crash_report**: cast signal handler to pointer before usize conversion `52d314b3`

All notable changes to this project will be documented in this file.

## [0.1.0] - 2026-03-26

### 📋 Release Summary

This release introduces comprehensive crash diagnostics, smart tab hibernation under memory pressure, and full-page screenshot capture with clipboard support (b1730570, 7c55614f, 1d3ef8e7). Enhanced navigation includes Safari-style deferred tab swapping, persistent search history with improved ranking, and new keyboard shortcuts for quick actions and tab jumping (2ba350ff, 1f8c1185, ae2d3eb1, 4395a238). Multiple stability improvements address WebContent crashes, media playback issues, and focus management across macOS and webview components (ce5d71e2, e278123e, d3700ab2, 628dcc94).


### ✨ New Features & Enhancements

- **search**: persist history and replace fuzzy filter with RRF ranking `1f8c1185`
- **crash**: add black-box crash diagnostics and reporting `b1730570`
- **ui**: add SPA same-document navigation support `e2e97791`
- **help**: add scroll and find shortcuts `44c1cb65`
- **url**: handle external app schemes on macOS `e5d8dd9f`
- **hibernation**: add smart tab hibernation under memory pressure `7c55614f`
- **screenshot**: add viewport and full page capture with clipboard `1d3ef8e7`
- **notification**: add download mode with auto-dismiss `90abffd2`
- **cold-open**: capture URLs delivered before event loop starts `fe4f2651`
- **mac**: register app as default web browser `f37ebf1f`
- **webview**: add find-in-page with CSS Custom Highlight API `11b2c7a5`
- **ui**: add page load stats to address bar with icons `8094bb5c`
- **ui**: show WebContent memory and CPU in address bar `1d1a227a`
- **webview**: enable devtools for debugging `0f908e84`
- **ui**: restore tab title and favicon on startup `fbbc5b82`
- **session**: restore tabs with titles and backward-compatible format `e5b50a1e`
- **ui**: add keyboard shortcuts overlay `946f37fd`
- **quickslots**: add save current page to empty slots `c7c890b5`
- **download**: add download started and completed events `a19636c4`
- **macos**: add block2 dependency for Objective-C blocks `77533891`
- **ui**: add new session button to restart chat with same agent `4ce2358d`
- **acp**: add tool tracking and cancellation `a10aa510`
- **notification**: add manual dismiss button and remove auto-dismiss `b8d52a23`
- **macOS**: add camera and microphone permissions for web media `fba87083`
- **mcp**: add tab management and page content tools `a3d42f19`
- **ui**: show ACP notifications when sidebar closed `3a92926d`
- **tabs**: add Safari-style deferred tab swap for smoother loading `2ba350ff`
- **search**: improve fuzzy matching with domain and word bonuses `1a6b1159`
- **ui**: add AskAI overlay with keyboard shortcuts `ec0f4613`
- **ui**: add keyboard shortcuts for URL quick-slots `4395a238`
- **ui**: add keyboard shortcuts ⌘1-⌘9 to jump to items `ae2d3eb1`

### 🔧 Improvements & Optimizations

- **shortcuts**: pair related shortcuts with slash separator `e09c129e`
- **overlay**: replace custom fuzzy filter with fuzzysort `b076527d`
- **find**: remove debounce and cache text nodes for faster search `9f648c1a`
- **shortcuts**: align shared key rows between columns `08d2b26b`
- **address-bar**: improve stats display formatting and layout `7ac30e66`
- **webview**: defer background tab creation `4d75359e`
- **sidebar**: adjust header height and padding `aa4015ed`
- **ui**: adjust progress bar and input alignment `771de648`
- **ui**: add macOS window corner radius to glass panels `2afbf157`
- **browser**: optimize history and URL handling `0b772ec1`
- **newtab**: replace flexbox with grid layout for slots `e2a3f697`
- **main**: extract macros for tab and UI operations `af588cd8`
- **ui**: convert sidebar to overlay window `a9db3dea`
- modularize codebase and add structured tracing `2e29a1ad`

### 🐛 Bug Fixes & Stability

- **macos**: disable automatic termination to prevent unwanted app quit `a4ad9c47`
- **browser**: defer URL assignment until navigation `efc447c3`
- **macOS**: re-enable CGEventTap after timeout and restore window focus `b2aae9f1`
- **stats**: prevent WKWebView leak in stats collection `7562cb63`
- **popup**: distinguish popup windows from regular new tab requests `2f047e5d`
- **keybindings**: require shift+cmd+q to quit `881313d9`
- **overlay**: correct keyboard jump shortcuts to use 1-0 for items 1-10 `63a345a5`
- **macos**: remove duplicate MRU updates on tab navigation `dbee91f5`
- **ui**: reserve space for footer bar in webview bounds `bb818c8d`
- **ui**: hide progress bar when switching tabs `3efd4d5f`
- **tab**: clear active_id on close and switch via MRU `a809ec50`
- **media**: handle WebContent crashes during playback `ce5d71e2`
- **ui**: show progress bar immediately on navigation `626cb9a5`
- **webview**: ensure WebKit repaints highlight before setting new position `8b7096d7`
- **find**: remove no matches display logic `f81801d7`
- **event-loop**: eliminate CPU spinning and WKWebView JS leaks `4b9cd408`
- **find**: clear highlights immediately on empty query `fa70dc78`
- **ui**: skip progress bar for about:blank pages `c3586e8e`
- **webview**: handle unicode characters in JS template escaping `f774183b`
- **nav**: ignore NSURLErrorCancelled during navigation `628dcc94`
- **focus**: prevent stealing focus when app is not active `d3700ab2`
- **clipboard**: enable copy in WKWebView via native pasteboard `66c66033`
- **sidebar**: restore keyboard focus when toggling visibility `3f61a4a5`
- **overlay**: add missing keyboard shortcuts for ask/search/url actions `33dda299`
- **quick-slot**: switch to existing tab instead of duplicating `0347b374`
- **notification**: reposition toast from center to right edge `281ea199`
- **tabs**: cancel deferred swap when switching tabs `761c98d5`
- **webview**: improve media tracking reliability for SPAs `e278123e`
- **macos**: import full user shell environment for .app bundles `b37de1cc`
- **macos**: expand PATH for .app bundle compatibility `0a29197d`

### 📚 Documentation & Examples

- change license from MIT to Apache 2.0 `293e3e2e`
- consolidate keyboard shortcut docs and sync README `f79efb2a`
- **mcp**: clarify tab targeting descriptions `54f95efc`
- **readme**: document command palette instant-jump shortcuts `32886091`
- **readme**: expand AI assistant and MCP server documentation `42f4f68b`
- expand architecture and module organization in INSTRUCTIONS `7646374d`

### 🔄 Other Changes

5 maintenance, dependency, and tooling updates not listed individually.
