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
- `crates/gif-core/src/writer.rs`의 `write_gif`가 팔레트 인덱스 프레임을 LZW로 압축해 GIF89a 파일로 씁니다. 두 번째 프레임부터는 바로 앞 프레임과 달라진 픽셀만 담습니다(`crates/gif-core/src/diff.rs`): 달라진 픽셀을 모두 감싸는 가장 작은 사각형만 쓰고, 그 안에서 안 변한 픽셀은 투명 번호로 채웁니다. 손실 압축은 이후 단위입니다.
- 투명 번호 한 자리를 비워 두어야 해서 팔레트는 최대 255색입니다(`Palette::MAX_COLORS`, `write_gif` 모두).
- `crates/gif-core/src/palette.rs`의 `Palette::from_frames`가 모든 프레임의 색을 중앙값 분할(median cut)로 최대 256색까지 줄여 전역 팔레트를 만들고, `to_indices`가 픽셀마다 가장 가까운 팔레트 색 번호를 고릅니다.
- `crates/gif-core/src/dither.rs`의 `ordered_dither`가 위치 고정 패턴(Bayer 8×8) 디더링을 합니다. 강도는 0..1이고 0이면 디더링 없음과 같습니다. 픽셀 하나가 바뀌어도 다른 픽셀의 결과는 바뀌지 않습니다. 오차 확산 방식은 아직 없습니다.
- `crates/gif-core/src/encode.rs`의 `encode_gif`가 RGB 프레임을 받아 팔레트 → 디더링 → 차분 → 압축을 한 번에 수행하는 순수 함수입니다. 설정은 `ManualSettings { colors: 2..=255, dither: 0..=1 }`입니다. `lossy`와 `maxSize` 모드는 아직 없고, 생기면 스케치의 `EncodeSettings` 합 타입으로 묶습니다.
- `app`(Tauri 2)과 `ui`(빌드 도구 없는 정적 HTML/JS)가 생겼습니다. 지금은 샘플 영상을 `encode_gif`로 만들어 창에 보여 주는 명령 `make_sample_gif` 하나뿐입니다. 화면이 커지면 TypeScript와 번들러를 도입합니다.
- `crates/gif-win`의 `ScreenCapturer`가 DXGI Desktop Duplication으로 모니터 하나를 열고, `capture(region)`이 그 영역의 RGB 픽셀을 돌려줍니다. 화면이 안 바뀌어 새 프레임이 없으면 마지막으로 받은 화면을 다시 씁니다. `ScreenRect`의 x, y는 그 모니터의 왼쪽 위를 (0, 0)으로 하는 물리 픽셀이고, `MonitorId`는 DXGI가 모니터를 나열하는 순서입니다. 실제 화면(2560×1440 주 모니터)에서 캡처되는 것을 확인했습니다. 복제를 연 직후의 첫 프레임은 `LastPresentTime`이 0이고 그림이 비어 있어서(전부 검정), 화면이 실제로 다시 그려진 프레임만 받아 둡니다. 회전된 모니터, `ACCESS_LOST` 복구, 해상도 축소는 아직 없습니다.
- `gif-core`의 `schedule::frame_due(fps, index)`가 녹화 시작부터 잰 각 프레임의 예정 시각을 돌려줍니다. `gif-win`의 `record_screen`은 프레임마다 그 시각까지 기다렸다가 캡처합니다. 간격을 더해 가지 않고 시작 시각에서 매번 다시 계산하므로 캡처가 늦어도 밀림이 쌓이지 않습니다. 루프는 프레임마다 기다린 뒤에 `keep_going()`을 묻고, 거짓이면 그 프레임을 찍지 않고 끝냅니다. 프레임 수 상한(`max_frames`)에 닿아도 끝납니다. 프레임은 모두 메모리에 둡니다.
- `gif-core`의 `selection::drag_region(monitor, monitor_size, from, to)`이 누른 점과 뗀 점을 녹화 영역으로 바꿉니다. 어느 방향으로 끌어도 같은 사각형이 되고, 모니터 밖으로 나간 부분은 가장자리에서 자르며, 한 변이 16픽셀보다 작으면 선택이 아닙니다(클릭은 무시).
- `gif-win`의 `select_region(monitor)`이 선택 오버레이입니다. 모니터 전체를 덮는 최상위 Win32 창(`WS_EX_LAYERED`)을 띄워 화면을 어둡게 하고, 드래그한 사각형은 색상 키로 뚫어 원래 밝기로 보여 줍니다. 왼쪽 버튼을 떼면 영역을 돌려주고, Esc나 오른쪽 클릭이면 `None`입니다. 이 함수는 호출한 스레드에서 메시지 루프를 돌며 끝날 때까지 기다립니다. 스레드를 Per-Monitor v2로 바꾼 뒤 창을 만들기 때문에 창 좌표가 곧 물리 픽셀이고, 좌표 변환은 여기에만 있습니다. 오버레이는 영역을 돌려주기 전에 없애므로 `WDA_EXCLUDEFROMCAPTURE` 없이도 첫 캡처에 찍히지 않습니다(테스트로 확인).
- 앱의 `Recorder`(`app/src/screen.rs`)가 녹화 한 번의 단계(`Idle → Selecting → Recording → StopAsked | CancelAsked → Encoding → Idle`)를 뮤텍스 하나로 소유합니다. 명령은 `record_screen_gif`(영역 선택부터 GIF까지 기다렸다가 돌려줌), `stop_recording`, `cancel_recording` 셋입니다. 드래그가 끝나면 바로 녹화가 시작되고(`immediate`의 시작 규칙), 그 순간 `recording-started` 이벤트로 영역 크기를 페이지에 알려 버튼이 [끝]으로 바뀝니다. 선택이나 녹화를 취소하면 빈 응답을 돌려줍니다. 녹화 중에 다시 시작하면 거절합니다. 프레임을 메모리에 두기 때문에 길이 상한은 30초와 "프레임 합계 1GiB" 중 짧은 쪽입니다(2560×1440, 30fps면 97프레임). 아직 주 모니터만 고를 수 있습니다. `Recorder`의 단계는 `gif-core`의 `SessionState`와 아직 따로입니다. `recordControl`·`selection` 설정이 들어올 때 합칩니다.
- `gif-win`의 `Hotkeys`가 전역 단축키를 맡습니다. 전용 스레드 하나가 `RegisterHotKey`로 등록하고 메시지 루프에서 `WM_HOTKEY`를 받아 콜백을 부릅니다. 등록과 해제는 등록한 스레드에서만 할 수 있어서, 다른 스레드의 `register`/`unregister` 호출은 채널에 요청을 넣고 그 스레드를 깨우는 방식입니다. `Hotkeys`를 버리면 스레드가 잡고 있던 단축키를 모두 놓습니다. `KeyCombo::from(Hotkey)`가 설정값을 Win32 수식키 비트와 가상 키 코드로 바꿉니다.
- 앱은 시작할 때 `Win+Shift+G`를 등록하고, 눌리면 페이지에 `begin-selection` 이벤트를 보내 [녹화 시작]과 같은 경로를 탑니다(설정값이 페이지 폼에 있기 때문). 등록에 실패하면 페이지에 그 사실을 표시하고 버튼으로만 시작합니다. Esc는 선택을 시작할 때, Enter는 녹화가 시작될 때 등록하고 끝나면 둘 다 해제합니다. 그래서 녹화하지 않을 때는 다른 프로그램의 Enter와 Esc를 가로채지 않습니다. 선택 중의 Esc는 `cancel_selection()`이 오버레이 창에 닫기 메시지를 보내 취소합니다(오버레이가 키보드 포커스를 못 얻은 경우에도 동작). 단축키 변경 UI는 아직 없습니다.
- 녹화한 GIF는 인코딩이 끝나면 `사진\midi\midi-YYYYMMDD-HHMMSS.gif`로 자동 저장됩니다. `gif-core`의 `output::gif_file_name`이 이름을 정하고(같은 초에 또 저장하면 `-2`, `-3`…을 붙여 기존 파일을 덮어쓰지 않음), 앱의 `save::save_gif`가 `.part` 파일에 쓴 뒤 이름을 바꿔서 쓰다 만 GIF가 남지 않게 합니다. 저장 결과는 `gif-saved` 이벤트로 페이지에 알리고, 실패해도 미리보기는 그대로 보여 줍니다. [폴더에서 보기]는 탐색기에서 그 파일을 선택해 엽니다. 저장 폴더를 바꾸는 설정과 클립보드 복사는 아직 없습니다. 샘플 GIF는 저장하지 않습니다.
- 폼의 선택(fps, 크기, 화질 단계, 용량 목표, 디더링 강도)은 `%APPDATA%\com.midi.app\settings.json`에 저장되고 다음 실행 때 폼에 다시 채워집니다. 저장하는 값은 녹화를 시작할 때 Rust로 보내는 `settings::RecordSettings`와 같은 타입이라, 저장용 형식이 따로 없습니다. 폼의 값이 바뀔 때마다 페이지가 `remember_settings`를 부르고, `settings::store`가 `.part` 파일에 쓴 뒤 이름을 바꿉니다. 앱을 열면 페이지가 `saved_settings`로 받아 폼에 채웁니다. 기본값(30fps, 영역 그대로, 자동, 용량 목표 없음, 디더링 1)은 `RecordSettings::default()` 한 곳에 있습니다. `settings::load`는 파일이 없거나, JSON으로 읽히지 않거나, 지금의 규칙으로 녹화를 시작할 수 없는 값(`can_record()`가 거짓)이면 통째로 기본값을 돌려주고, 파일에 없는 항목만 그 항목의 기본값으로 채웁니다. 녹화를 시작할 수 없는 값(예: 가로 0)은 저장하지 않으므로 파일에는 마지막으로 쓸 수 있었던 설정이 남습니다. 크기와 화질 단계를 둘 다 정해 용량 목표 칸이 꺼진 동안에는 용량 목표를 "없음"으로 저장합니다. 단축키와 저장 폴더는 아직 설정이 아니어서 저장하지 않습니다.
- 녹화 길이의 한도는 앱의 `screen::recording_limit(fps, 저장 크기)`가 정합니다: 30초와 "프레임이 메모리 1GiB에 들어가는 길이" 중 짧은 쪽이고, 어느 쪽이 정했는지를 `LimitedBy::{Time, Memory}`로 함께 돌려줍니다(예: 480×270은 어느 fps에서나 30초, 2560×1440에 30fps는 97프레임, 약 3.2초). 녹화가 시작되면 `recording-started` 이벤트에 한도(밀리초)와 그 이유를 실어 보내고, 페이지는 남은 시간을 초 단위로 표시합니다. 메모리 때문에 30초보다 짧으면 처음부터 "이 크기와 fps에서는 최대 N초"를 같이 보여 줍니다. 녹화가 누구의 요청도 없이 끝났으면(루프가 끝났는데 상태가 아직 `Recording`) 한도에 닿은 것이므로 `Recorded { encoded, ended_at_limit: true }`를 돌려주고, 페이지는 결과 아래에 그 사실과 이유를 적습니다. 메모리 한도였으면 "크기를 줄이거나 fps를 낮추면 더 길게"도 안내합니다. Enter 단축키로 끝낸 경우에는 페이지가 그 순간을 알지 못해, 인코딩이 끝날 때까지 남은 시간 표시가 이어집니다. 프레임을 디스크에 쓰는 방식(스풀)과 최대 길이 설정은 아직 없습니다.
- 인코딩 시간, 색 찾기 표를 넣기 전(2026-11-22 측정, 릴리스 빌드, `cargo run --release -p gif-win --example encode_time -- 가로 세로 fps 프레임수 [still|scroll]`): 처리량은 화질 1단계에서 초당 약 2.6메가픽셀, 6단계에서 약 10~14메가픽셀이고 크기와 무관하게 일정합니다. 300×200 30초(30fps, 900프레임)는 1단계 약 21초·6단계 약 5초, 1280×720 10초(15fps)는 약 54초·14초, 2400×1400 3.5초(30fps)는 약 136초·34초입니다. 1단계 시간의 약 90%가 디더링 단계(픽셀마다 팔레트에서 가장 가까운 색 찾기)이고, 그 시간은 팔레트 색 수에 거의 비례합니다(255색 120초, 32색 18초). 나머지는 팔레트 고르기 약 4%, 차분과 압축 약 7%(화면이 안 바뀌면 1% 미만), 잡음 제거 1% 미만입니다. 화면이 전혀 바뀌지 않아도 디더링 시간은 같습니다(바뀌지 않은 픽셀도 다시 계산). 모든 단계는 코어 하나만 씁니다. 진행률 표시는 아직 없습니다.
- 색 찾기 표: `Palette::lookup()`이 돌려주는 `NearestColor`가 RGB 색 1,677만 개마다 한 칸(1바이트, 합계 16MB)인 표를 들고, 처음 물어본 색만 `Palette::nearest`로 팔레트 전체를 훑어 답을 적어 둡니다. 다시 물어본 색은 표에서 바로 읽습니다. 답은 `nearest`가 낸 것 그대로이므로 GIF의 내용은 표를 넣기 전과 같습니다. 팔레트는 255색까지라 번호 255가 "아직 안 물어봄" 표시입니다. `ordered_dither`는 이 표를 받고, `encode_gif`는 표 하나를 모든 프레임에 씁니다. 같은 예제로 다시 잰 결과(스크롤, 1단계): 300×200 30초는 21초 → 3.7초, 1280×720 10초는 54초 → 11초, 2400×1400 3.5초는 136초 → 23초이고, 디더링 단계만 보면 120초 → 2.8초입니다. 1단계와 6단계의 시간 차이는 거의 없어졌습니다. 이제 남은 시간은 차분과 압축 약 52%, 팔레트 고르기(모든 픽셀의 색 세기) 약 33%, 디더링 약 12%입니다. 화면이 안 바뀌는 녹화에서는 팔레트 고르기가 약 65%입니다. 측정할 때마다 화면 내용이 달라 파일 크기는 서로 비교할 수 없습니다.
- **크기** 설정: `gif-core`의 `resize::OutputSize`는 `Original`(드래그한 영역 그대로) 또는 `FitWithin { width, height }`입니다. `for_source(width, height)`가 비율을 유지한 채 지정한 가로×세로 안에 들어가는 가장 큰 크기를 돌려줍니다(800×500을 480×270 안에 넣으면 432×270). 영역이 이미 더 작으면 키우지 않습니다. `resize::downscale`이 출력 픽셀 하나에 대응하는 원본 픽셀 묶음의 평균(반올림)으로 줄이고, 묶음은 `index × 원본 ÷ 출력`으로 나눠서 원본 픽셀이 빠지거나 겹치지 않습니다. `record_screen`이 프레임을 찍자마자 줄여서 보관하므로 메모리 상한(1GiB)과 인코딩 시간도 출력 크기를 따릅니다. 가로×세로로 지정한 크기는 용량 목표가 있어도 바뀌지 않습니다. "드래그한 영역 그대로"로 두면 용량 목표를 맞추려고 인코딩 단계에서 더 줄어들 수 있습니다.
- 열화 정도(`ManualSettings.tolerance`, 0~32)는 화질 단계가 정하는 값입니다(폼에 따로 나오지 않음). `gif-core`의 `stabilize::stabilize(frames, tolerance)`가 각 픽셀을 **지금 화면에 보이는 색**과 비교해서, 세 채널 모두 차이가 tolerance 이하면 화면의 색을 그대로 둡니다. 바로 앞 프레임이 아니라 화면의 색과 비교하므로 조금씩 계속 변하는 픽셀도 차이가 tolerance를 넘는 순간 따라갑니다. `encode_gif`가 팔레트를 만들기 전에 이 단계를 거치고, 디더링이 위치 고정이라 같은 색은 같은 번호가 되어 프레임 차분이 그 픽셀을 투명으로 처리합니다. gifsicle의 lossy LZW와는 다른 방식입니다(그쪽은 압축 단계에서 비슷한 번호열을 재사용). 실제 화면 480×270, 15fps 3초 녹화에서 0 → 272,680바이트, 4 → 123,687, 8 → 102,767, 16 → 78,377바이트였습니다. - 용량 목표가 생겼습니다. `gif-core`의 `fit::EncodeSettings`가 스케치의 합 타입입니다: `Manual(ManualSettings)` 또는 `MaxSize { max_bytes, dither }`. `fit::encode`가 둘을 받아 `Encoded { gif, width, height, fitted }`를 돌려줍니다. `MaxSize`는 화질 단계 10개(`LADDER`: 크기 100%·255색·열화 0부터 25%·16색·열화 24까지)에서 목표 이하가 되는 가장 좋은 단계를 찾습니다. 먼저 최고 단계를 해 보고(맞으면 한 번으로 끝), 다음에 최저 단계를 해 보고(그래도 크면 그 결과를 `fits: false`로 돌려줌), 그 사이를 이분 탐색하므로 인코딩은 많아야 6번입니다. 단계가 낮을수록 파일이 작다고 가정합니다. 크기 단계는 메모리의 프레임을 `downscale`로 다시 줄이므로 출력 크기가 녹화 해상도보다 작아질 수 있습니다. 앱 폼의 [용량 목표](없음, 1, 2, 5, 8, 10MB)가 이 설정이고, 목표를 고르면 색 수와 열화 정도 슬라이더는 쓰이지 않습니다(디더링 강도는 쓰임). 어느 단계로 만들었는지와 목표를 못 맞춘 경우를 `gif-fitted` 이벤트로 페이지에 표시합니다.
- **화질 단계**와 **용량 목표**: `gif-core`의 `fit::LEVELS`가 화질 단계 6개입니다(1단계 255색·열화 0, 2단계 255색·열화 4, 3단계 255색·열화 8, 4단계 128색·열화 8, 5단계 64색·열화 12, 6단계 32색·열화 16). `fit::Quality`는 `Level(1..=6)`(용량 목표 없음) 또는 `WithinBytes { max_bytes, adjust }`입니다. `adjust`는 용량을 맞추려고 무엇을 바꿔도 되는지를 뜻합니다: `Level`(크기 고정, 단계를 내림), `Size { level }`(단계 고정, 크기를 줄임), `LevelThenSize`(둘 다 자동, 단계를 6단계까지 내린 뒤에 크기를 줄임). 크기는 100%에서 90, 80, 70, 60, 50, 40, 33, 25%로 줄이고 25%가 하한입니다. `adjust`마다 "좋은 것부터 나쁜 것까지"의 후보 목록을 만들고(최대 14개), 같은 `first_fitting` 탐색으로 목표 이하인 가장 좋은 후보를 고릅니다. 먼저 첫 후보를 해 보고(맞으면 한 번으로 끝), 다음에 마지막 후보를 해 보고(그래도 크면 그 결과를 `within_limit: Some(false)`로 돌려줌), 그 사이를 이분 탐색하므로 인코딩은 많아야 6번입니다. 뒤 후보일수록 파일이 작다고 가정합니다. `fit::encode`는 `Encoded { gif, level, width, height, within_limit }`를 돌려줍니다. 앱의 `choice::QualityChoice::settings(size_is_fixed)`가 폼의 값을 이 타입으로 바꿉니다: 크기를 가로×세로로 지정했으면 크기 고정, 단계를 직접 골랐으면 단계 고정이고, 둘 다 고정이면 조정할 것이 없으므로 폼에서 용량 목표 칸을 끄고 Rust도 이유와 함께 거절합니다. 샘플 GIF는 크기를 자동으로 봅니다. 단계 목록은 `quality_levels` 명령으로 Rust에서 받아 폼에 채웁니다. 결과 아래에 실제 크기(줄였으면 `300×200 → 180×120`)와 쓰인 단계를 표시하고, 끝까지 낮춰도 목표를 넘으면 그 사실과 함께 "더 짧게 녹화"(크기 고정이면 "크기를 줄이거나"도)를 안내합니다. 색 수와 열화 정도를 직접 조절하는 슬라이더는 없앴습니다. 디더링 강도는 남아 있습니다.
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
| 2026-11-20 | 팔레트는 최대 255색 + 투명 1칸(프레임 차분용) | Claude 제안, 사용자 확정 |
| 2026-11-20 | 코드는 `/pstack:teach`로 설명할 수 있는 작은 단위로 쌓아 감(단위마다 하나의 개념, 테스트, 왜 그렇게 했는지 기록) | 사용자 |
| 2026-11-22 | 화질 설정은 [크기], [화질 단계], [용량 목표] 세 가지. 단계는 사용자가 직접 고를 수 있고, 크기는 가로×세로로 지정 | 사용자 |
| 2026-11-22 | 지정한 가로×세로와 영역의 비율이 다르면 비율을 유지한 채 그 안에 들어가게 줄임(늘이거나 자르지 않음) | Claude 제안, 사용자 확정 |
| 2026-11-22 | 지정한 크기에서 최저 단계로도 용량 목표를 못 맞추면 알려 주고 그대로 저장(크기를 자동으로 줄이지 않음) | Claude 제안, 사용자 확정 |
| 2026-11-22 | 녹화한 GIF는 `사진\midi`에 시각 이름으로 자동 저장 | Claude 기본값, 사용자 확인 대기 |
| 2026-11-22 | 용량 목표를 고르면, 크기와 화질 단계 중 사용자가 정한 것은 고정하고 정하지 않은 것을 자동으로 조정. 둘 다 자동이면 화질을 먼저 내리고 그다음 크기를 줄임. 둘 다 정했으면 용량 목표를 쓸 수 없음 (같은 날의 "크기를 자동으로 줄이지 않음"은 크기를 직접 지정한 경우로 좁혀짐) | Claude 제안, 사용자 확정 |
| 2026-11-22 | 크기를 자동으로 줄일 때의 하한은 원래 크기의 25%. 그래도 넘으면 안내 | Claude 제안, 사용자 확정 |
| 2026-11-22 | 폼의 선택은 바꿀 때마다 `%APPDATA%\com.midi.app\settings.json`에 저장하고 다음 실행 때 되살림. 읽을 수 없거나 쓸 수 없는 값이면 기본값으로 시작 | Claude 기본값, 사용자 확인 대기 |

처음에 권장했던 "시작 단축키 토글"은 채택하지 않았습니다. 녹화 중 조작이 없는 용도라서 Enter가 시연 조작과 충돌할 일이 없고, 커서를 찍지 않으니 버튼 방식의 단점(버튼으로 가는 커서가 찍힘)도 사라졌기 때문입니다.

## 6. 미결정

- DRM 보호 영상(넷플릭스 등, 브라우저 하드웨어 가속 재생)은 캡처하면 검은 화면이 나옴. 우회하지 않고 안내만 할지
- 출력 파일 이름 규칙
