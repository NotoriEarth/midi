# GIF 영역 캡처: 설계 스케치

상태: 초안. 용도, 커서, 시작/종료/취소 방식은 확정(2026-11-20, 사용자 결정). 구현 스택은 Tauri 2(Rust 코어 + 웹 UI)로 확정.
대상 OS: Windows 10 19045 이상.
**용도**: 재생 중인 영상의 일부를 잘라 짤(GIF)로 만드는 것. 녹화하는 동안 영역 안에서 조작은 없다고 가정.

## 1. 사용자 흐름

시작과 종료 방식은 설정의 `recordControl`로 고르며, 시작 방식마다 짝이 되는 종료 방식이 정해져 있습니다.

```
                          ┌─ immediate ─────────────────────────→ Recording ─(Enter)─┐
Idle ─(선택 시작)→ Selecting ─(드래그 완료)┤                                                     ├→ Encoding → Done
                          └─ button ─→ Armed ─([시작] 클릭)→ Recording ─([끝] 클릭)───┘
         ▲                │ Esc            │ Esc              │ Esc (스풀 폐기)
         └────────────────┴────────────────┴──────────────────┘
```

| `recordControl` | 시작 | 종료 |
|---|---|---|
| `immediate` | 드래그를 마치는 순간 | Enter |
| `button` | 영역 밖에 뜨는 [시작] 버튼 클릭 | 같은 버튼이 [끝]으로 바뀜 → 클릭 |

**영역을 정하는 방식**은 설정의 `selection`으로 고릅니다.

| `selection` | 영역 정하기 |
|---|---|
| `drag` | 단축키 → 화면이 어두워짐 → 드래그 (위 흐름 그대로) |
| `frame` | 단축키 → 지난번 위치와 크기 그대로 **녹화 틀**이 뜸(Selecting을 건너뛰고 바로 Armed) → 틀의 상하좌우 가장자리를 끌어 크기를 바꾸거나 틀을 옮김 → [시작] 클릭 |

- `frame`에서는 드래그가 없으므로 `recordControl`이 `immediate`여도 시작은 [시작] 버튼입니다. 종료는 `recordControl`을 따릅니다(`immediate`면 Enter, `button`이면 [끝]).
- Armed 동안에는 틀을 옮기거나 크기를 바꿀 수 있고(`RegionChanged`), 녹화가 시작되면 영역은 고정됩니다. `drag` + `button` 조합에서도 Armed 동안 같은 방식으로 영역을 다듬을 수 있습니다.
- 틀은 영역 바깥에 그리는 테두리와 손잡이로 이루어지고 `WDA_EXCLUDEFROMCAPTURE`로 캡처에서 빠집니다. 틀 안쪽은 클릭이 통과해서 영상 플레이어를 그대로 조작할 수 있습니다.

- Esc는 인코딩 전의 모든 단계(Selecting, Armed, Recording)에서 취소입니다. 두 모드 공통이고, Recording에서 취소하면 쌓인 프레임을 버리고 Idle로 돌아갑니다.
- `button` 모드에서는 [끝] 옆에 [✕] 버튼도 둡니다(마우스로 쓰는 모드라서).
- 최대 녹화 길이(`maxDurationSec`)에 도달하면 종료한 것과 똑같이 인코딩합니다. 녹화를 잊어버렸을 때를 위한 안전장치입니다.
- 결과는 파일로 저장하고, 설정에 따라 클립보드에 복사합니다(CF_HDROP).
- **커서는 GIF에 찍지 않습니다.**

## 2. 이 기능이 건드리는 시스템

| 시스템 | 결정해야 할 것 / 함정 |
|---|---|
| 선택 시작 진입점 | 앱이 트레이에 떠 있는 동안 전역 단축키(`beginSelectionHotkey`, 기본 `Win+Shift+G`)를 누르면 바로 선택 오버레이가 뜸. Windows의 Win+Shift+S와 같은 사용감. `RegisterHotKey`가 실패하면(다른 프로그램이 이미 사용 중) 알리고 설정에서 바꾸게 함. 호스트 앱의 버튼으로도 시작 가능 |
| 선택 오버레이 | 최상위 투명 창. 모니터별 DPI(Per-Monitor v2): 논리 좌표와 물리 좌표 변환을 여기서 딱 한 번만 함 |
| 화면 캡처 | DXGI Desktop Duplication. DXGI는 커서를 원래 포함하지 않으므로 커서 제외는 공짜. 화면이 바뀔 때만 프레임이 오므로 타임스탬프로 프레임 간격을 정함 |
| Enter 종료 (`immediate`) | 녹화 중에만 수정키 없는 `RegisterHotKey(VK_RETURN)`을 등록하고, 끝나면 해제. **Enter가 영상 플레이어로 가지 않고 가로채짐**(일부 플레이어는 Enter가 전체 화면 전환이라 중요함) |
| Esc 취소 | Selecting, Armed, Recording 동안 `RegisterHotKey(VK_ESCAPE)`를 등록. 포커스가 영상 플레이어에 있어도 동작함. 플레이어의 Esc(전체 화면 해제 등)도 가로채므로 녹화 중 화면이 갑자기 바뀌지 않음 |
| [시작]/[끝] 버튼 (`button`) | 클릭 통과가 아닌 작은 최상위 창. `WDA_EXCLUDEFROMCAPTURE`로 캡처에서 빠지므로 영역과 겹쳐도 GIF에 찍히지 않음. 영역 밖에 둘 자리가 없으면(전체 화면 영역) 영역 안쪽 모서리에 둠 |
| 녹화 중 표시 | 영역 바깥 테두리. 클릭 통과 + `WDA_EXCLUDEFROMCAPTURE` |
| 프레임 저장 | 해상도 축소는 캡처 단계에서 적용. 메모리 상한을 넘으면 임시 파일로 넘김 |
| GIF 인코딩 | 영상은 그라데이션이 많아서 디더링 품질이 체감 화질을 좌우함. 카메라가 움직이면 프레임 간 차분 효과가 작아 용량이 큼 → 최대 용량 모드가 중요. 라이선스 주의: gifski는 AGPL, ImageSharp는 매출 조건부 상용 라이선스 |
| 설정 | 녹화 시작 시점의 설정을 스냅샷으로 떠서, 녹화 도중 바꿔도 진행 중인 세션에는 영향 없음 |

## 3. 두 가지 형태

### A. 녹화 → 봉인 → 일괄 인코딩 (선택)
캡처한 프레임을 스풀에 쌓기만 하고, 종료하면 봉인한 뒤 인코더가 전체를 한 번에 처리합니다.
- 전체 프레임을 보고 전역 팔레트를 만들 수 있어 화질이 가장 좋음
- 최대 용량 모드가 가능함: 결과가 크면 설정을 낮춰 다시 인코딩
- 인코더가 순수 함수라서 테스트하기 쉬움
- 대신 종료 후 인코딩 대기 시간이 있음(진행률 표시로 보완)

### B. 스트리밍 인코딩
프레임이 들어오는 즉시 인코딩해, 종료 직후 바로 결과가 나오는 방식입니다. 이미 쓴 프레임은 되돌릴 수 없어서 최대 용량을 보장할 수 없고, 프레임별 지역 팔레트를 쓰게 되어 화질도 떨어집니다.

**A를 고른 이유**: "용량 설정" 요구사항은 B로는 정직하게 구현할 수 없습니다.

## 4. 타입 스케치 (스택 중립 의사코드)

```ts
// 호스트 앱이 아는 유일한 표면
interface GifCaptureFeature {
  beginSelection(): void          // 호스트의 단축키나 버튼이 호출. Idle일 때만 의미 있음
  readonly state: Observable<SessionState>
}

type SessionState =
  | { kind: 'idle' }
  | { kind: 'selecting' }
  | { kind: 'armed'; region: ScreenRect }                    // button 모드에서만 거침
  | { kind: 'recording'; region: ScreenRect; startedAt: Instant; frameCount: number }
  | { kind: 'encoding'; progress: number }                   // 0..1
  | { kind: 'done'; file: OutputFile }
  | { kind: 'failed'; error: CaptureError }

// UI 부품(오버레이, Enter 핫키, 버튼)은 이벤트만 보낸다. 상태는 Session만 바꾼다.
type SessionEvent =
  | { kind: 'regionSelected'; region: ScreenRect }
  | { kind: 'startClicked' }
  | { kind: 'stop' }              // Enter든 [끝]이든 최대 길이 도달이든 같은 이벤트
  | { kind: 'cancel' }            // Esc 또는 [✕]. selecting, armed, recording에서 유효

// 물리 픽셀 기준, 한 모니터에 고정. v1에서는 여러 모니터에 걸친 영역을 시작 모니터 경계로 잘라냄.
type ScreenRect = { monitor: MonitorId; x: number; y: number; width: number; height: number }

type RecordControl = 'immediate' | 'button'   // 시작과 종료가 짝으로 묶여 있으므로 설정 하나

type GifSettings = {
  beginSelectionHotkey: Hotkey   // 수정키가 하나 이상 필요. Enter와 Esc는 고를 수 없음 (구현: crates/gif-core/src/hotkey.rs)
  selection: 'drag' | 'frame'    // frame이면 마지막 틀 위치와 크기도 함께 저장
  recordControl: RecordControl
  capture: { fps: GifFps; scale: number; maxDurationSec: number }
  encode: EncodeSettings
  output: { dir: Path; copyToClipboard: boolean }
}

// GIF 프레임 지연은 1/100초 단위이고, 브라우저는 0.02초 미만 지연을 0.1초로 늘려 버림 → 최대 50fps
// 설정에서 고를 수 있는 값은 10, 15, 30 (구현: crates/gif-core/src/fps.rs)
type GifFps = 10 | 15 | 30

// "열화 정도"와 "용량"은 동시에 지정할 수 없음 → 둘 중 하나만 고르는 합 타입
type EncodeSettings =
  | { mode: 'manual'; colors: number /*16..256*/; dither: number /*0..1*/; lossy: number /*0..100*/ }
  | { mode: 'maxSize'; maxBytes: number }   // 결과가 크면 lossy → colors → scale → fps 순으로 낮춰 재인코딩

interface FrameSource {                    // 구현은 DXGI 하나
  open(region: ScreenRect, opts: GifSettings['capture']): FrameStream
}
interface FrameStream { next(): Frame | 'end'; close(): void }
type Frame = { pixels: RgbaImage; timestampMs: number }   // scale 적용 완료, 커서 없음

interface FrameSpool {
  append(f: Frame): void
  seal(stopAtMs: number): SealedFrames     // stop 시각 이후 프레임은 버림
  discard(): void                          // cancel 시 메모리와 임시 파일을 모두 정리
}

// 순수 함수. 프레임 지연은 반올림 오차를 누적해 보정함(30fps → 3,3,4 / 100초 반복)
function encodeGif(frames: SealedFrames, s: EncodeSettings, onProgress: (p: number) => void): Promise<Bytes>
```

### 구현 현황

- `crates/gif-core/src/session.rs`의 `step(state, event, control)`이 위 전이 규칙을 순수 함수로 구현합니다. 해당하지 않는 이벤트는 상태를 바꾸지 않습니다.
- 스케치와 다른 점: 선택 시작을 `BeginSelection` 이벤트로 넣었습니다(스케치에서는 `beginSelection()` 호출).
- `crates/gif-core/src/writer.rs`의 `write_gif`가 팔레트 인덱스 프레임을 GIF89a 파일로 씁니다. 지금은 압축하지 않습니다(LZW 사전이 커지기 전에 매번 초기화 코드를 넣는 방식). 실제 LZW 압축, 프레임 차분, 팔레트 양자화는 이후 단위입니다.
- 아직 없는 것: `done`, `failed` 상태, `recording`의 시작 시각과 프레임 수, `encoding`의 진행률, 효과 목록. 캡처와 인코더 단위에서 추가합니다.

### 상태별 소유자 (하나의 상태에 소유자는 하나)

| 상태 | 소유자 |
|---|---|
| 세션 단계 | `Session` 상태 머신. 다른 부품은 `SessionEvent`만 보냄 |
| 시작/종료 방식 | `GifSettings.recordControl` 하나. 세션은 스냅샷을 보고 Armed를 거칠지 정함 |
| 전역 핫키 등록 | `Session`이 상태에 들어가고 나갈 때 등록/해제. Enter는 Recording(immediate 모드만), Esc는 Selecting~Recording. 어떤 경로로 나가든(stop, cancel, failed) 해제가 보장되어야 함 |
| 설정 | `SettingsStore`. 세션은 시작할 때 스냅샷을 받음 |
| 캡처 장치 | `FrameStream` (`close`로 해제) |
| 프레임 | `seal` 전에는 `FrameSpool`, 봉인 후에는 인코더가 읽기만 함 |
| 논리/물리 좌표 변환 | 선택 오버레이 |

## 5. 결정 기록

| 날짜 | 결정 | 결정한 사람 |
|---|---|---|
| 2026-11-20 | 용도는 영상 짤 만들기(녹화 중 조작 없음) | 사용자 |
| 2026-11-20 | 커서는 GIF에 찍지 않음 → `drawCursor` 설정 삭제 | 사용자 |
| 2026-11-20 | 시작/종료는 `immediate`(드래그 → Enter)와 `button`([시작] → [끝]) 중 설정에서 선택 | 사용자 |
| 2026-11-20 | 녹화 도중 취소(버리기) 기능 필요 | 사용자 |
| 2026-11-20 | 취소 방식: 두 모드 공통 Esc, `button` 모드는 [✕] 버튼도 추가 | Claude 제안, 사용자 승인 |
| 2026-11-20 | 최종 목표는 ezgif 같은 범용 편집 도구. 이 문서의 화면 캡처는 그 안의 추가 기능 | 사용자 |
| 2026-11-20 | 구현 스택: Tauri 2 (Rust 코어 + 웹 UI). 근거는 [ezgif-analysis.md](ezgif-analysis.md) | Claude 제안, 사용자 확정 |
| 2026-11-20 | 설정에서 고를 수 있는 fps는 10, 15, 30 (이전 스케치의 20, 25, 50은 뺌) | 사용자 |
| 2026-11-20 | 전역 단축키를 누르면 바로 영역 선택이 시작됨(Win+Shift+S와 같은 방식). 단축키는 설정에서 변경 가능 | 사용자 |
| 2026-11-20 | 기본 단축키는 `Win+Shift+G` | Claude 제안, 사용자 확정 |
| 2026-11-20 | 드래그 외에, 상하좌우로 크기를 조절하는 녹화 틀로 영역을 정하는 방식을 설정에서 고를 수 있음 | 사용자 |
| 2026-11-20 | 녹화 중에는 영역 고정(틀 이동과 크기 변경 불가) | Claude 제안, 사용자 확정 |
| 2026-11-20 | 녹화 틀 방식의 시작은 항상 [시작] 버튼, 종료는 `recordControl`을 따름 | Claude 제안, 사용자 확정 |
| 2026-11-20 | 코드는 `/pstack:teach`로 설명할 수 있는 작은 단위로 쌓아 감(단위마다 하나의 개념, 테스트, 왜 그렇게 했는지 기록) | 사용자 |

처음에 권장했던 "시작 단축키 토글"은 채택하지 않았습니다. 녹화 중 조작이 없는 용도라서 Enter가 시연 조작과 충돌할 일이 없고, 커서를 찍지 않으니 버튼 방식의 단점(버튼으로 가는 커서가 찍힘)도 사라졌기 때문입니다.

## 6. 미결정

- DRM 보호 영상(넷플릭스 등, 브라우저 하드웨어 가속 재생)은 캡처하면 검은 화면이 나옴. 우회하지 않고 안내만 할지
- 출력 파일 이름 규칙
