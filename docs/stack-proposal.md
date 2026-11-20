# GIF 영역 캡처: 구현 스택 제안

상태: **종결**. 2026-11-20 사용자가 Tauri 2(Rust 코어 + 웹 UI)로 확정. 이 문서의 C#/WPF 추천과 §5 프로젝트 구조는 채택되지 않았습니다.

> **2026-11-20 전제 변경**: 최종 목표가 ezgif 같은 범용 편집 도구로 바뀌었습니다. 이 문서의 §1 추천(C#/WPF)은 캡처 기능만 놓고 낸 것이라 더는 유효하지 않습니다. 새 추천은 [ezgif-analysis.md](ezgif-analysis.md) §5를 보세요. §2 공통 구조, §4 인코더, §6 스파이크는 그대로 유효합니다.
근거: [gif-capture-design.md](gif-capture-design.md). 이 문서는 네 개의 후보 제안서를 비교·종합한 결과입니다(맨 끝 "종합 노트" 참고). 크기와 속도 수치는 측정하지 않은 추정입니다.

## 1. 결론

**추천: C# / .NET 10 + WPF 호스트 + CsWin32 + Vortice + GIF 인코더 직접 구현.**

단, 다음 중 하나에 해당하면 **Rust**(호스트는 Tauri 2 또는 egui)가 더 낫습니다.
- 이미 Rust에 익숙하다
- 런타임 설치 없이 5~15MB짜리 작은 exe로 배포하는 게 중요하다

두 선택지 사이에서 바뀌는 건 **호스트 앱의 언어와 UI뿐**입니다. 아래 §2의 내부 구조는 어느 쪽이든 같습니다.

## 2. 스택과 무관하게 네 후보가 모두 합의한 구조

1. **오버레이, 녹화 테두리, [시작]/[끝]/[✕] 버튼은 UI 프레임워크를 쓰지 않고 Win32 창으로 직접 만듭니다.** 모니터마다 창을 하나씩 둡니다. DPI 인식 방식을 Per-Monitor v2로 선언한 프로세스에서는 Win32 창 좌표가 처음부터 물리 픽셀이므로, 설계 문서의 "좌표 변환은 한 곳에서만"이 사실상 "변환 0번"이 됩니다. WPF나 Qt, 웹뷰로 만들면 각자 논리 좌표 체계가 한 겹 더 끼어서 125%, 150% 같은 배율에서 좌표가 어긋납니다.
2. **Enter/Esc는 프레임워크의 단축키 기능을 쓰지 않고 `RegisterHotKey`를 직접 호출합니다.** `UnregisterHotKey`는 등록한 스레드에서만 성공합니다. 그래서 핫키, 오버레이, 버튼 창은 모두 **같은 UI 스레드 하나**가 소유해야 합니다.
3. **GIF 인코더는 직접 구현하고, 다른 코드에 의존하지 않는 독립 모듈로 둡니다.** 허용형 라이선스(MIT/Apache 등) 중에는 프레임 차분과 `lossy`까지 갖춘 인코더가 없습니다(§4). 그래서 어느 스택이든 인코더 핵심은 직접 짜게 됩니다.
4. **상태 머신은 순수 함수와 효과 실행기로 나눕니다.**
   - 순수 함수: `(상태, 이벤트, 설정 스냅샷) → (다음 상태, 실행할 효과 목록)`
   - 효과 실행기: 그 목록을 받아 창 띄우기, 캡처 시작 같은 실제 Win32 호출을 수행
   - 이렇게 나누면 모든 상태와 이벤트 조합을 Windows 없이 표로 테스트할 수 있습니다.
5. **첫 번째 스파이크(실현 가능성을 먼저 확인하는 짧은 실험)는 같습니다.** 레이어드 창에 `WDA_EXCLUDEFROMCAPTURE`를 걸었을 때 Desktop Duplication 결과에서 정말 빠지는지 확인합니다.

## 3. 대안 비교

| | **A. C# + WPF (추천)** | B. Rust + Tauri 2 | C. Rust + egui | D. C++ + Qt 6 |
|---|---|---|---|---|
| 언어 | C# 하나 | Rust + TypeScript | Rust 하나 | C++ 하나 |
| Win32/DXGI 접근 | CsWin32(MS 공식) + Vortice, 쉬움 | `windows` crate, `unsafe` 많음 | B와 같음 | COM 직접 호출, 가장 자연스러움 |
| 호스트 UI 생산성 | 높음(XAML) | 높음(웹), 단 Rust↔웹 통신 비용 | 중간(즉시 모드 UI, 복잡한 화면에 약함) | 중간 |
| 한글 입력/네이티브 컨트롤 | 좋음 | 좋음 | 보통(미검증) | 좋음 |
| 인코딩 1회(추정) | Rust 대비 1.3~2배 느림 | 빠름 | 빠름 | 빠름 |
| 배포 크기 | 수 MB + .NET 런타임 설치, 또는 런타임 포함 60~100MB | 5~15MB + WebView2 | 3~10MB, 런타임 없음 | 30~60MB 폴더 |
| 의존성 라이선스 | 전부 MIT | 전부 MIT/Apache | 전부 MIT/Apache | Qt LGPL(단일 exe로 만들기 어려움) |
| 학습 비용 | 낮음 | 높음 | 높음 | 높음 |
| 대표 위험 | 배포 크기, 인코더 속도 | 언어 두 개 + 빌드 체인 두 벌 | 트레이 상주와 창 수명 관리가 매끄럽지 않음 | 빌드 복잡도, Qt DPI 층, 라이선스 |

**검토했다가 뺀 것**
- **Electron**: 화면 캡처, 클립보드, 인코딩을 전부 네이티브 애드온으로 따로 짜야 합니다. 그 위에 100MB가 넘는 런타임까지 얹힙니다.
- **WinUI 3**: 투명 창과 클릭 통과 창을 만들기 불편하고, 배포가 무겁습니다.
- **Avalonia**: 크로스플랫폼 UI 프레임워크인데, 이 기능은 Windows 전용이라 얻는 게 없습니다.
- **Python**: 인코더 속도가 부족하고, 배포 파일이 크며, 백신이 악성으로 오탐하는 경우가 잦습니다.

**WPF를 고른 경우 주의할 점**
- WPF는 Native AOT(미리 컴파일해 런타임 없이 실행)와 트리밍(안 쓰는 코드 제거)을 공식 지원하지 않습니다. 그래서 런타임을 포함한 배포가 작아지지 않습니다.
- .NET 8 LTS는 2026-11-10에 지원이 끝났으므로 .NET 10 LTS(2028-11까지 지원)로 시작합니다.

## 4. GIF 인코더

| 라이브러리 | 라이선스 | 판단 |
|---|---|---|
| gifski | AGPL-3.0 또는 상용 | 화질 최고. 쓰면 프로젝트 전체가 AGPL 조건을 따라야 함 → **기본은 끔, 별도 빌드 옵션으로만**(§4.3) |
| libimagequant / `imagequant` | GPL-3.0 또는 상용 | 쓰지 않음 |
| gifsicle | GPL-2.0 | 쓰지 않음(lossy LZW는 개념만 참고하고 코드는 보지 않고 직접 구현) |
| ImageSharp | Six Labors Split License(매출 조건부 상용) | 쓰지 않음 |
| FFmpeg palettegen/paletteuse | LGPL(빌드 구성에 따라 다름) | `lossy` 없음. 직접 구현이 막혔을 때의 예비안 |
| Magick.NET | Apache-2.0 | 네이티브 바이너리가 수십 MB이고 반복 인코딩에 느림 |

### 4.1 파이프라인

```
SealedFrames
  → (maxSize의 scale/fps 단계면 스풀에 저장된 프레임을 다시 축소하거나 솎아냄)
  → 히스토그램(서브샘플링) → Wu 양자화(colors-1개 + 투명 인덱스 1개)
  → 프레임마다 병렬:
       차분 마스크: 직전 출력 프레임과 거의 같은 픽셀은 디더링하기 전에 투명으로 확정
       변경 사각형 안에서만 팔레트 매핑 + 디더링
       LZW(lossy=0이면 표준, 아니면 오차를 허용하는 방식)
  → GIF89a 기록(지연 시간은 반올림 오차를 누적해 보정: 30fps → 3,3,4)
```

### 4.2 영상에서 중요한 두 가지

- **디더링 기본값은 위치 고정 패턴(Bayer 또는 blue-noise)입니다.** 오차 확산(Floyd–Steinberg)은 프레임마다 노이즈 무늬가 달라집니다. 그러면 화면이 지글거리고, 프레임 차분이 거의 먹히지 않아 용량이 커집니다. 오차 확산은 옵션으로만 둡니다.
- **디더링은 변경 영역 안에서만 합니다.** 그래야 카메라가 고정된 장면에서 용량이 줄어듭니다.

### 4.3 교체 지점

- `IQuantizer`(색상 양자화)와 `IDitherer`(디더링)를 인터페이스로 둡니다. 나중에 프로젝트 라이선스가 GPL로 정해지면 `imagequant` 같은 구현을 끼울 수 있습니다.
- gifski 백엔드는 같은 `EncodeGif` 시그니처 뒤에 둡니다. C# 쪽에서는 gifski의 C API를 P/Invoke로 호출합니다. **기본은 꺼 두고, 프로젝트를 AGPL로 정했을 때만 켭니다.**

### 4.4 maxSize 탐색 방식

- 팔레트는 `colors` 값이 바뀔 때만 다시 만듭니다.
- 매번 전체를 인코딩하지 않고, 일부 프레임(예: 5프레임 중 1개)만 인코딩해서 최종 용량을 추정한 뒤 매개변수를 이분 탐색합니다.
- 마지막에 전체 인코딩을 한 번 해서 실제로 상한을 지키는지 확인합니다.
- 이렇게 하면 전체 인코딩 횟수가 3~6회로 묶입니다.

## 5. 추천안 프로젝트 구조

```
midi/
├─ midi.sln
├─ Directory.Build.props              # net10.0-windows, Nullable, TreatWarningsAsErrors
├─ src/
│  ├─ Midi.App/                       # WPF exe. 기능들을 조립만 하는 껍데기
│  │  ├─ App.xaml(.cs)                # 단일 인스턴스, 서비스 구성, 기능 등록, 트레이
│  │  ├─ app.manifest                 # PerMonitorV2
│  │  ├─ Shell/TrayMenu.cs, SettingsWindow.xaml
│  │  └─ Composition.cs               # 기능 목록: new GifCaptureFeatureModule(), ...
│  ├─ Midi.Host.Abstractions/         # 기능이 아는 호스트 표면(인터페이스만, 의존성 없음)
│  │  └─ IFeatureModule, IHostServices, IHotkeyService, ISettingsStore, INotifier
│  ├─ Midi.Platform.Windows/          # 공용 Win32 부품. CsWin32의 NativeMethods.txt는 여기 하나
│  │  └─ MessageWindow, HotkeyService, TrayIcon, LayeredWindow, CaptureExclusion,
│  │     Monitors, FileDropClipboard, JsonSettingsStore(schemaVersion, 원자적 저장)
│  ├─ Midi.Gif/                       # 순수 .NET. Win32와 WPF 참조 금지
│  │  ├─ Frames/  Frame, FrameSpool(Append/Seal/Discard), SealedFrames
│  │  ├─ Settings/EncodeSettings.cs   # abstract record { Manual, MaxSize } + [JsonPolymorphic]
│  │  └─ Encoding/ GifEncoder, MaxSizeSearch, DelayPlanner, IQuantizer/WuQuantizer,
│  │               IDitherer/OrderedDither/ErrorDiffusionDither, FrameDiff,
│  │               LzwEncoder, LossyLzwEncoder, Gif89aWriter
│  ├─ Midi.Capture.Dxgi/              # FrameSource 구현은 이것 하나
│  │  └─ DxgiFrameSource, DxgiFrameStream, AdapterForMonitor, GpuCropScale
│  └─ Midi.Features.GifCapture/       # 기능 모듈. 위 부품들을 조립
│     ├─ GifCaptureFeatureModule.cs, GifCaptureFeature.cs
│     ├─ Session/ SessionState, SessionEvent, Transition(순수 함수), SessionEffect,
│     │           HotkeyPolicy(순수 함수), Session
│     ├─ Capture/ IFrameSource, RecordingPump
│     ├─ Ui/      SelectionOverlay, RecordButtonWindow, RecordingBorder
│     ├─ Settings/GifSettings.cs, GifSettingsPage.xaml
│     └─ Output/OutputWriter.cs
├─ tests/
│  ├─ Midi.Gif.Tests/                 # 지연 보정, maxSize 단계 순서, 디코드 왕복, 차분
│  ├─ Midi.Features.GifCapture.Tests/ # 모든 상태×이벤트 전이 표, HotkeyPolicy, 스냅샷 격리
│  └─ Spikes/                         # WDA+레이어드, ACCESS_LOST, 혼합 DPI, 전체 화면 플레이어
└─ tools/GifBench/                    # BenchmarkDotNet + 실제 녹화 샘플로 maxSize 루프 시간 측정
```

**참조 방향**
- `App`은 `Features.*`와 `Platform.Windows`를 참조합니다.
- `Features.GifCapture`는 `Host.Abstractions`, `Gif`, `Capture.Dxgi`, `Platform.Windows`를 참조합니다.
- `Midi.Gif`는 아무것도 참조하지 않습니다. 그래서 인코더를 Rust로 바꿀 때 이 경계에서 통째로 교체할 수 있습니다.

### 5.1 설계 문서의 불변식을 지키는 방법

| 불변식 | 구현 |
|---|---|
| 상태는 `Session`만 바꿈 | UI 부품은 이벤트를 보내는 함수(`Action<SessionEvent>`)만 받습니다. 전이는 UI 스레드에서 하나씩 처리합니다 |
| 핫키는 어떤 경로로 나가든 해제됨 | `HotkeyPolicy.For(state, recordControl)`가 지금 등록돼 있어야 할 핫키 집합을 돌려줍니다. `Session`은 전이할 때마다 실제 등록 상태를 그 집합에 맞춥니다. 나간 경로가 아니라 현재 상태만 보고 정하므로, 정상 종료·취소·오류 어디로 나가도 해제됩니다. 예외가 나면 `finally`에서 대기 상태 기준으로 다시 맞춥니다. 이 모든 일이 UI 스레드에서 일어나므로 스레드 문제도 없습니다 |
| 설정 스냅샷 | `GifSettings`를 불변 `sealed record`로 만듭니다. 세션 시작 시점의 값을 그대로 들고 있으면 됩니다 |
| 봉인 후에는 읽기만 | `Seal(stopAtMs)`이 읽기 전용 `SealedFrames`를 돌려주고, 이후 `Append`는 예외를 던집니다 |
| 인코딩 진행률 | 인코더 스레드에서 `IProgress<double>`로 UI 스레드에 전달해 `Encoding.Progress`를 갱신합니다. 공개 `SessionEvent`에는 추가하지 않는 **내부 경로**입니다 |

### 5.2 스레드

| 스레드 | 하는 일 |
|---|---|
| UI(WPF Dispatcher) | `Session`, 모든 Win32 창의 메시지 처리, `WM_HOTKEY`, 트레이, 설정 창 |
| 캡처 전용 | `DxgiFrameStream` → `FrameSpool.Append`, 프레임 수는 100ms마다 UI로 전달 |
| 스레드 풀 | `EncodeGif`(내부에서 `Parallel.For`) |

타임스탬프는 QPC(고해상도 타이머) 기준 밀리초 하나로 통일합니다. DXGI의 `LastPresentTime`과 종료 시각 모두 같은 기준을 씁니다.

### 5.3 다른 기능이 붙는 방법

1. 새 프로젝트 `Midi.Features.<이름>`을 만들고 `IFeatureModule`을 구현합니다.
2. `Composition.cs`에 한 줄을 추가합니다.
3. 기능이 쓸 수 있는 것은 호스트 서비스로 제공합니다: 전역 단축키, 트레이 메뉴, 설정 섹션, 설정 탭, 알림.
4. 전역 단축키는 `HotkeyService` 하나가 소유하므로 기능끼리 키가 충돌하면 한 곳에서 감지합니다.

## 6. 착수 전 스파이크

각각 반나절에서 이틀 정도입니다.

1. **캡처 제외 확인**: 레이어드·클릭 통과 테두리와 버튼 창에 `WDA_EXCLUDEFROMCAPTURE`를 걸었을 때 Desktop Duplication 결과에서 빠지는지. 함께 확인할 것은 하이브리드 GPU 노트북에서 모니터에 맞는 어댑터 찾기, `ACCESS_LOST`(해상도 변경 등) 복구입니다.
2. **Enter/Esc 가로채기**: 전체 화면 영상 플레이어에 포커스가 있을 때도 수정키 없는 Enter/Esc 등록이 동작하고 키가 플레이어로 가지 않는지. 다른 프로그램이 같은 키를 이미 등록해 실패하는 경우의 처리도 정합니다.
3. **혼합 DPI 오버레이**: 배율이 다른 모니터 두 대에서 오버레이 좌표가 정확한지.
4. **인코더 벤치(`tools/GifBench`)**:
   - 측정 조건: 10초, 30fps, 640×360(300프레임)
   - 판정: maxSize 루프 전체가 8코어 기준 **5초 이하**면 C# 인코더를 유지합니다. 넘으면 무거운 반복 부분(팔레트 매핑·디더링·LZW)만 Rust DLL로 옮기고, 공개 시그니처는 그대로 둡니다.
   - 5초는 임의로 정한 기준이니 조정하셔도 됩니다.
   - 이 벤치 결과로 디더링 기본값(Bayer 또는 blue-noise)도 확정합니다.

## 7. 설계 문서에 반영할 후보 (미승인)

종합 과정에서 드러난 설계 문서의 빈틈입니다. **사용자가 승인하기 전까지는 설계 문서를 고치지 않습니다.**

1. **`lossy`의 의미**: "LZW 단계에서 오차를 허용한다"(gifsicle 방식)인지, "직전 프레임과 색 차이가 임계값 이하면 투명 처리한다"(차분 허용치)인지 정해지지 않았습니다. 이 제안서는 둘 다 쓰는 쪽(차분 임계값과 손실 LZW 강도를 함께 조절)을 가정했습니다.
2. **스풀 크기**: 1280×720 BGRA를 15fps로 30초(450프레임) 쌓으면 약 1.66GB이고, 640×360이면 약 415MB입니다. 메모리 상한(예: 512MB)을 넘으면 디스크로 넘기는 현재 구조는 유지하되, RGB24로 저장하면 25% 줄어듭니다.
3. **maxSize의 scale 단계**: 캡처할 때 이미 축소된 스풀 프레임을 한 번 더 축소하는 것이라는 점을 명시합니다.
4. **핫키 등록 실패**: 다른 프로그램이 Enter/Esc를 이미 쓰고 있으면 `failed`로 보낼지, 안내만 띄우고 버튼으로 끝내게 할지 정해야 합니다.
5. **인코딩 진행률**: 공개 `SessionEvent`에 넣지 않고 내부 경로로 처리합니다(§5.1).

---

## 종합 노트

**과정**: 네 모델(Opus, Fable, Sonnet, Haiku)이 같은 과제를 각자 받아 제안서를 썼습니다. 그다음 다른 계열 모델(Fable)이 다섯 기준으로 교차 심사했습니다. 기준은 플랫폼 API 커버리지, 인코더, 호스트 적합성과 배포, 1인 개발 속도, 구체성과 정확성입니다. 이후 제가 네 제안서를 끝까지 읽고 기준별로 따로 채점했습니다.

**기준으로 삼은 후보: candidate-1(Opus, C#/WPF)**
- 교차 심사(Fable)와 제 채점이 모두 이 후보를 골랐습니다.
- 핫키 해제를 "현재 상태 → 있어야 할 핫키 집합"이라는 순수 함수로 맞추는 방식이라, 스레드 문제가 없고 표로 테스트할 수 있습니다.
- 인코더 계획이 가장 완성돼 있습니다(손실 LZW 직접 구현, 샘플 기반 maxSize 탐색, 수치로 된 판정 기준과 Rust로 옮기는 탈출구).
- 사실 오류가 가장 적었고, 일부 주장은 웹에서 확인했습니다.

**다른 후보에서 가져온 것**
| 출처 | 가져온 것 |
|---|---|
| candidate-2 (Fable, Tauri) | 위치 고정 디더링을 기본값으로, gifski를 기본은 꺼 둔 별도 빌드 옵션으로, 전체 화면 플레이어에서 Enter/Esc 가로채기 스파이크 |
| candidate-3 (Sonnet, C#/WPF) | `IQuantizer` 교체 지점(디더러까지 확장), `lossy` 의미를 미결로 올림, 설정 파일 `schemaVersion`, `tests/Spikes/` |
| candidate-4 (Haiku, Tauri) | 인코딩 진행률을 내부 경로로, 스풀 크기 분석(숫자는 바로잡음), maxSize scale 단계 명시, 핫키 등록 실패 처리 |

**가져오지 않은 것과 이유**
- **Tauri 추천(candidate-2, 4)**: 이 기능의 UI 부품은 전부 Win32 창이라 웹뷰의 이점이 이 기능에는 없습니다. 언어 두 개와 빌드 체인 두 벌의 부담만 남습니다. 다만 "Rust에 익숙하다면"이라는 조건부 대안으로 §1에 남겼습니다.
- **candidate-2의 핫키 구조**: 핫키 해제 가드를 별도 runner 스레드가 들고 있다가 해제하는 구조인데, `UnregisterHotKey`는 등록한 스레드에서만 성공하므로 그대로는 동작하지 않습니다.
- **candidate-4의 오류들**: 핫키 가드를 상태 값 안에 넣으면서 전이 함수가 순수하다고 주장하는 모순, .NET 8(지원 종료) 기준, 스풀 크기 계산 오류(약 5GB라고 했지만 실제는 약 1.66GB).
- **candidate-3의 `lossy` 재정의**: `lossy`를 차분 허용치로 바꿔 정의한 것은 채택하지 않고, 사용자가 정할 질문으로 올렸습니다(§7-1).

**수렴**: 호스트 스택은 2 대 2로 갈렸습니다. 그 아래 구조(§2)는 네 후보가 완전히 같았습니다. 따라서 실제로 남은 결정은 **개발자가 어떤 언어에 익숙한지와 배포 크기가 얼마나 중요한지**입니다.

**탈락한 후보**: 없음(4/4 완료).

**검증**
- 직접 계산해 확인: 스풀 크기(1280×720×4×450 = 1,658,880,000바이트, 640×360은 414,720,000바이트), 설계 문서 절 번호 참조.
- 교차 심사와 대조: .NET 8 지원 종료일(2026-11-10), `WDA_EXCLUDEFROMCAPTURE` 최소 버전(Win10 19041), `UnregisterHotKey`의 스레드 제약. 이 세션에서 공식 문서를 직접 다시 열어 보지는 않았습니다.
- 라이브러리 라이선스는 후보 제안서를 근거로 했습니다. 일부는 candidate-1이 웹에서 확인했습니다.
- 속도 수치는 측정하지 않았습니다(§6-4에서 측정 예정).
