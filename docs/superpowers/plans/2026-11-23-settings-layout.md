# 설정 화면 다시 배치하기: 구현 계획

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** midi 창(720×600)에서 페이지 스크롤 없이 녹화 결과가 위에 크게, 설정이 아래 두 열로 보이게 한다.

**Architecture:** 정적 페이지 세 파일(`ui/index.html`, `ui/style.css`, `ui/main.js`)만 바꾼다. 마크업과 CSS는 고른 시안을 옮기고, 스크립트는 [샘플 만들기] 관련 줄만 지운다. 확인은 개발 빌드를 CDP로 띄워 실제 WebView2에서 잰다.

**Tech Stack:** Tauri 2(WebView2), 순수 HTML/CSS/JS, Node 내장 `fetch`/`WebSocket`으로 CDP.

**Spec:** `docs/superpowers/specs/2026-11-23-settings-layout-design.md`, 시안 `docs/superpowers/specs/2026-11-23-settings-layout-mockup.html`

## Global Constraints

- 창 720×600 CSS px에서 `document.scrollingElement.scrollHeight <= 600`, `scrollWidth <= 720`.
- 글씨 13px, 안내 12px, 제목 14px. `prefers-color-scheme`으로 밝게·어둡게.
- 지금 페이지의 id를 모두 유지(`#make`만 삭제). 폼 `#settings`는 녹화 설정만 감싸고, `#hotkey-input`·`#hotkey-default`·`#autostart`는 폼 밖.
- 설정 항목, 값(`value`), 동작은 바꾸지 않는다. 문구만 시안대로.
- 앱 확인 스크립트는 다른 midi-app이 켜져 있으면 시작하지 않고, 자기가 띄운 PID만 끈다. 셸의 `%APPDATA%`는 Claude 격리 사본이라 셸에서 띄운 앱끼리만 같은 설정을 본다.

## Review Focus

1. 녹화 중 상태(`#cancel` 보임, `#record` 문구가 "끝"으로 바뀜): 머리줄이 넘치거나 버튼이 밀리지 않아야 함. Task 1 확인에 "녹화 중" 상태를 넣는다.
2. 저장된 설정이 "가로×세로 지정 + 직접 입력"일 때 처음 열기: 조건부 칸이 펼쳐진 채로도 스크롤 없음. Task 1 확인에 이 상태를 넣는다.
3. 결과가 없을 때(앱을 막 켰을 때): 미리보기 자리에 빈 상태 문구가 보이고, 저장 경로·인코딩 줄은 숨음. Task 1 확인에 넣는다.
4. 화질 단계 선택지가 Rust에서 채워진 뒤(`quality_levels`) 가장 긴 문구("6단계 (16색, 열화 40)")로 선택칸이 잘리지 않음. Task 1에서 선택칸 `scrollWidth <= clientWidth`를 본다.
5. 아주 긴 상태 문구: 페이지는 스크롤 없이, "녹화 결과" 칸 안에서만 스크롤. Task 1에서 긴 문구를 넣고 페이지 `scrollHeight`와 `.result`의 `overflow-y: auto`를 본다.

---

### Task 1: 화면 배치 확인 스크립트 (먼저 실패를 본다)

**Files:**
- Create: `tools/check-ui-layout.mjs`
- Create: `tools/check-ui-layout.sh`

**Interfaces:**
- Produces: `node tools/check-ui-layout.mjs <cdp-port>`: 앱 페이지에 붙어 아래 항목을 확인하고, 하나라도 틀리면 이유를 출력하고 종료 코드 1. 모두 맞으면 `layout ok` 출력, 종료 코드 0.
- Produces: `bash tools/check-ui-layout.sh`: midi-app이 켜져 있으면 "another midi-app is running" 출력 후 종료 코드 2. 아니면 개발 빌드(`target/debug/midi-app.exe`)를 `WEBVIEW2_ADDITIONAL_BROWSER_ARGUMENTS=--remote-debugging-port=9352`로 띄우고, 위 스크립트를 돌린 뒤 자기가 띄운 PID만 끈다.

- [ ] **Step 1: 확인 항목을 스크립트로 쓴다**

`check-ui-layout.mjs`는 CDP `Emulation.setDeviceMetricsOverride`로 720×600을 맞추고, `Runtime.evaluate`로 페이지를 상태별로 바꿔 가며 잰다(상태는 DOM을 직접 바꿈: `hidden` 토글, 텍스트 대입, 선택값 바꾼 뒤 `change` 이벤트 발생).

| 상태 | 바꾸는 것 | 기대 |
|---|---|---|
| 처음 연 그대로 | 없음 | `#preview` 숨김, 빈 상태 문구 보임, 스크롤 없음 |
| 결과 있음 | `#preview.src`에 637×421 data URI, `#preview`·`#saved`·`#encoded` 보이기, 문구 대입 | 스크롤 없음 |
| 조건부 칸 모두 | `#selection`=frame, `#size-mode`=fit, `#max-size`=custom 후 `change` | `#size-box`, `#max-size-box`, `#record-control-hint` 보임, 스크롤 없음 |
| 녹화 중 | `#cancel` 보이기, `#record` 문구 "끝" | 스크롤 없음, `#record`와 `#cancel`이 창 안 |
| 긴 문구 | `#status`에 200자, `#encoded`에 120자 | 페이지 스크롤 없음, `.result`의 계산된 `overflow-y`가 `auto` |

공통 확인: `scrollHeight <= 600`, `scrollWidth <= 720`; 지금 id 목록(`settings fps selection record-control record-control-hint size-mode size-box size-width size-height level max-size max-size-box max-size-custom max-size-hint dither-value dither record cancel hotkey hotkey-input hotkey-default autostart status encoded saved saved-path show-saved preview`)이 모두 있음; `#make` 없음; `#settings` 안에 `#autostart`·`#hotkey-input` 없음; `#level`·`#size-mode`·`#max-size`의 `scrollWidth <= clientWidth`.

- [ ] **Step 2: 지금 페이지로 돌려 실패를 본다**

Run: `cargo build -p midi-app && bash tools/check-ui-layout.sh`
Expected: 종료 코드 1, 출력에 `scrollHeight` 600 초과와 `#make still present`가 있음.

- [ ] **Step 3: 커밋**

```bash
git add tools/check-ui-layout.mjs tools/check-ui-layout.sh
git commit -m "Add a check that the settings page fits a 720x600 window"
```

### Task 2: 시안대로 페이지 바꾸기

**Files:**
- Modify: `ui/index.html` (전체)
- Modify: `ui/style.css` (전체)
- Modify: `ui/main.js` (`make` 상수, `make.disabled` 네 곳, 폼 `submit` 처리)

**Interfaces:**
- Consumes: Task 1의 `bash tools/check-ui-layout.sh`

- [ ] **Step 1: 마크업과 CSS를 시안에서 옮긴다**

시안 `docs/superpowers/specs/2026-11-23-settings-layout-mockup.html`의 `<main class="app">`과 `<style>`을 옮긴다. 지울 것: `.demo` 스타일, `<details class="demo">`, 시안 `<script>`, 테마 바꾸기용 `[data-theme]` 규칙(있으면). 미리보기 `src`(data URI)는 지운다. `hidden` 기본값은 지금 페이지대로: `#record-control-hint`, `#size-box`, `#max-size-box`, `#cancel`, `#encoded`, `#saved`, `#preview`에 `hidden`. 시안의 선택된 값(`selected`, `disabled`, `checked`, 입력 `value`)은 지금 페이지의 기본값으로 되돌린다(fps 10/15/30 중 기본은 main.js가 저장값으로 채움, `#size-width` 480, `#size-height` 270, `#max-size-custom` 3, `#dither` 1). `#level`에는 "자동 (용량 목표가 없으면 1단계)" 하나만 둔다. `<script src="main.js">`를 남긴다. 상태 첫 문구 "설정을 고르고 버튼을 누르세요."는 "설정을 고르고 [녹화 시작]을 누르세요."로 바꾼다.

- [ ] **Step 2: main.js에서 샘플 관련 줄을 지운다**

`const make = …`, `make.disabled = …` 네 줄, `form.addEventListener("submit", …)` 블록을 지운다. 폼은 `novalidate`이고 submit 버튼이 없으므로 Enter로 제출되지 않는다(폼 안 입력에서 Enter를 눌러도 페이지가 다시 읽히지 않는지 Step 4에서 본다).

- [ ] **Step 3: 확인 스크립트를 다시 돌린다**

Run: `cargo build -p midi-app && bash tools/check-ui-layout.sh`
Expected: `layout ok`, 종료 코드 0.

- [ ] **Step 4: 실제 동작을 CDP로 본다**

같은 방식으로 띄운 앱에서: `#fps`를 10으로 바꾸고 `change`를 보낸 뒤 `invoke("saved_settings")`의 `fps`가 10인지 확인하고 원래 값으로 되돌린다. `#size-width`에서 Enter 키 이벤트를 보내도 `location.href`가 그대로인지 본다. 밝게·어둡게(`Emulation.setEmulatedMedia`의 `prefers-color-scheme`) 두 장을 찍어 눈으로 본다.

- [ ] **Step 5: 커밋**

```bash
git add ui/index.html ui/style.css ui/main.js
git commit -m "Lay out the settings page with results on top and settings below"
```

### Task 3: 설치판으로 확인하고 기록

**Files:**
- Modify: `docs/gif-capture-design.md` (구현 현황에 한 줄)

- [ ] **Step 1: 설치판을 만들어 덮어 설치한다**

사용자가 켠 midi가 있으면 종료를 부탁하고 기다린다. Run: `cd app && cargo tauri build`, 이어서 설치판을 `/S`로 실행. Expected: 설치 종료 코드 0, 설치된 `midi-app.exe` 시각이 새 빌드.

- [ ] **Step 2: 사용자가 PC에서 본다**

녹화 한 번, 결과가 위에 크게 나오는지, 설정을 바꿔도 저장되는지, 스크롤이 없는지 사용자에게 확인을 부탁한다.

- [ ] **Step 3: 설계 문서에 적고 커밋**

`docs/gif-capture-design.md`의 구현 현황에 "설정 화면: 위 결과·아래 설정 두 열, 720×600에서 스크롤 없음(`tools/check-ui-layout.sh`)" 한 줄과 확인 결과를 적는다.

```bash
git add docs/gif-capture-design.md
git commit -m "Record the new settings layout"
```
