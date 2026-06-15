# Changelog

## [0.8.0] - 2026-06-15

### 📋 Release Summary

This release introduces a comprehensive overhaul of the user interface and an interactive agent surface system, featuring improved tool persistence, customizable keybindings, and native macOS fullscreen support (17433748, b950d73d, 92eebeaf, 3f7e99de). Significant enhancements were made to browser automation, MCP integration, and accessibility, including a new editable address bar and Web Speech API support (41f898a3, 3a880bda, 90d458f0, 6b4f14e9). Additionally, several bug fixes improve webview focus, layout stability, and overall system reliability (2f1290eb, 07ff2e89, fd941f93).


### ✨ New Features & Enhancements

- **mcp**: actionability harness, observability, dialogs/uploads, e2e suite `6b4f14e9`
- **browser**: enhance MCP browser automation and observability `41f898a3`
- **browser**: enhance navigation and interaction reliability `e644b298`
- **ui**: overhaul visual styling and tool persistence `17433748`
- **sandbox**: confine agent filesystem access to workspace `660a134a`
- **keybindings**: implement configurable global shortcuts `b950d73d`
- **ui**: change command palette shortcut to ⌘⇧P `b63c3832`
- **mcp**: improve tab management and navigation `211e9a1f`
- **sidebar**: add clickable tool rows and live modal updates `85cd136b`
- **sidebar**: move input hints to placeholder `fb863cfe`
- **address-bar**: implement editable URL and autocomplete `3a880bda`
- **ui/core**: overhaul iconography and memory management `4f307771`
- **web**: enable Web Speech API recognition `90d458f0`
- **mcp**: add callback watchdogs and retry logic `a05af4cb`
- **browser**: implement SPA readiness probe and stability monitoring `6e1450c2`
- **ui**: implement robust A2UI surface routing and persistence `7b9cf9e3`
- **macos**: implement native and sidebar fullscreen modes `92eebeaf`
- **ui**: persist A2UI surface snapshots and improve markdown `4397a458`
- **sidebar**: add rich UI renderers for slash commands `31a2f97f`
- **sidebar**: implement v0.9 UI component specification `2b8b4d52`
- **ui**: implement A2UI interactive agent surface system `3f7e99de`

### 🔧 Improvements & Optimizations

- unify formatting and improve readability `e526450f`
- **github**: migrate rust jobs to shared workflow `f9fd4c24`
- **release**: migrate to shared release workflow `8e1b0ef4`
- **macos**: improve shortcut handling and cleanup `d77dbdd3`
- **workflow**: migrate pr brief to reusable workflow `97cb24b3`
- **readiness**: reformat rustdoc comments for clarity `1eee611d`
- **sidebar**: modernize command output UI `de6e78c2`

### 🐛 Bug Fixes & Stability

- **ui**: restore keyboard focus to active webview `2f1290eb`
- **ui**: set quickslots bar height to 36px `25d41d11`
- **webview**: reset bounds when showing tabs `07ff2e89`
- **ui**: improve inline edit positioning and layering `17e175e4`
- **mcp**: clarify error for dropped browser calls `3792045c`
- **ui**: prevent duplicate bubbles and preserve timestamps `fd941f93`
- **ui**: improve out-of-band event handling and A2UI rendering `f89242ae`
- **sidebar**: resolve scalar items in list templates `5398bb0a`

## [0.7.0] - 2026-05-14

### 📋 Release Summary

This release introduces persistent chat session history and enhanced sidebar navigation, featuring new keyboard shortcuts, slash command autocomplete, and a welcome screen for empty sessions (69d34008, 60b191ac, d41e1935). macOS users will benefit from improved native integration, including local network permissions, synchronous JS dialogs, and more reliable window focus management (9e2a5728, 80408d66, 73ce5445). Stability is further bolstered by refined input handling, optimized memory polling, and several fixes to message processing and UI responsiveness (7bdb182f, 92110051, 7fd6203b).


### ✨ New Features & Enhancements

- **macos**: add location and local network permissions `9e2a5728`
- **macos**: implement native synchronous JS dialogs `80408d66`
- **sidebar**: increase session limit and add tab navigation `1e41bd57`
- **mcp**: enhance DOM interaction reliability and tool diagnostics `265b9182`
- **ui**: add Ctrl+J shortcut for newlines `d03dc99b`
- **macos**: restore sidebar focus on app reactivation `73ce5445`
- **acp**: enforce message history limits on load and push `d3f7d892`
- **acp**: implement persistent chat session history `69d34008`
- **webkit**: enable WebAuthn and passkey support `05f413f9`
- **mcp**: auto-restore hibernated tabs for commands `cf33659e`
- **sidebar**: add Ctrl+J shortcut for newlines `52939423`
- **sidebar**: track per-message tool details `814d428c`
- **acp**: persist and resume session ids `4d0e8e4d`
- **sidebar**: add welcome screen for empty sessions `d41e1935`
- **sidebar**: globalize session cycling shortcut `bfcf0eba`
- **ai**: implement persistent history for sidebar prompts `f67a0a53`
- **sidebar**: add keyboard shortcuts for sessions `a268de74`
- **sidebar**: isolate prompt history and session state `c7c92158`
- **sidebar**: add command output card display `8fb39c58`
- **sidebar**: add slash command autocomplete `605b91ac`

### 🔧 Improvements & Optimizations

- **main**: throttle memory polling and lazy-load sessions `7bdb182f`
- **mcp**: remove unused tool router field `4b194ec5`
- **github**: add automated pull request briefing job `659c8a8b`
- **sidebar**: unify prompt history across sessions `c4cca880`

### 🐛 Bug Fixes & Stability

- **macos**: prevent duplicate text input in WKWebView `92110051`
- **sidebar**: prevent message send on modified Enter `c1d6a83a`
- **macos**: resign key window on app deactivation `711c52fb`
- **sidebar**: improve Ctrl+J newline behavior `95333fd2`
- **macos**: use NSWorkspace PID for active state check `54c1790d`
- **build**: correct bundle identifier and signing `6aeafab1`
- **macos**: replace cached focus state with active app query `60c81d85`
- **sidebar**: auto-close unclosed markdown fences `6b1ebb52`
- **acp**: implement force-kill for wedged agents `ddd99e4c`
- **acp**: extend idle timeout and improve error reporting `b8b6532c`
- **sidebar**: decouple agent busy state from spinner UI `49ea687c`
- **sidebar**: restore caret position on session switch `017caa04`
- **sidebar**: enable parallel message processing and cleanup legacy files `7fd6203b`
- **ui**: delegate window dragging to native macOS `5b5735a2`
- **input**: prevent quit trigger when ctrl is held `6f04c5a6`
- **sanitize**: handle # before ? in URLs `e258c66a`

### 🔄 Other Changes

4 maintenance, dependency, and tooling updates not listed individually.

## [0.6.0] - 2026-04-21

### 📋 Release Summary

This update introduces advanced browser automation for dialogs and element interaction alongside RAM-aware hibernation for optimized resource management (f653f374, 2803aea0, 71c90203). Security and privacy are strengthened with new data sanitization measures to prevent sensitive information leakage in prompts and outputs (eebd5cf4, 5c067f32). Reliability is further improved through refined window and popup handling and streamlined core logic (debfe68c, 05b45c46, 3ae246e2, 65c7a996).


### 🚨 Breaking Changes

⚠️ **Important**: This release contains breaking changes that may require code updates.

- **mcp**: remove browser_get_html tool `f653f374`

### ✨ New Features & Enhancements

- **hibernation**: add RAM-aware proactive thresholds `71c90203`
- **mcp**: add browser automation tools for JS dialogs and element interaction `2803aea0`

### 🔧 Improvements & Optimizations

- **core**: simplify logic and event handling `3ae246e2`
- **sidebar**: simplify code renderer parameters `65c7a996`

### 🐛 Bug Fixes & Stability

- **window**: route window.open to tabs or popups `debfe68c`
- **sanitize**: redact PANs in text outputs `eebd5cf4`
- **mcp**: prevent sensitive data leakage in prompts and snapshots `5c067f32`
- **webview**: preserve window.opener for popup windows `05b45c46`

## [0.5.2] - 2026-04-08

### 📋 Release Summary

This release introduces SPA title change detection for improved webview tracking. Enhanced PDF rendering and clipboard functionality provide a more seamless copy-paste experience. Documentation improvements help users better understand octoweb's features (279fb7b0, 577a72d2).


### ✨ New Features & Enhancements

- **webview**: add SPA title change detection `28f23f1f`

### 🔧 Improvements & Optimizations

- **clipboard**: use CGBitmapContext for PDF rendering and clipboard copy `577a72d2`

### 📚 Documentation & Examples

- enhance README with comprehensive feature documentation `279fb7b0`

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
