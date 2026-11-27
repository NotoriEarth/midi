# midi

화면의 한 영역을 녹화해서 GIF로 저장하는 Windows 트레이 앱입니다.

## 다운로드

[Releases](https://github.com/NotoriEarth/midi/releases)에서 가장 최근의 `midi_x.y.z_x64-setup.exe`를 받아 실행하세요.

## 요구 사항

- Windows 10 또는 11 (64비트)
- 관리자 권한은 필요 없습니다. 현재 사용자 계정에만 설치됩니다.
- WebView2 런타임이 없으면 설치하는 동안 내려받습니다. 이때 인터넷이 필요합니다.

## 설치할 때 나오는 SmartScreen 경고

설치 파일에 코드 서명이 없어서 Windows SmartScreen이 경고 창을 띄울 수 있습니다. **추가 정보**를 누른 뒤 **실행**을 누르면 설치가 계속됩니다.

## 사용법

1. midi를 실행하면 트레이에 아이콘이 생깁니다. 창을 닫아도 앱은 트레이에 남습니다.
2. 기본 단축키 `Win+Shift+G`를 누르거나 트레이 아이콘을 왼쪽 클릭하면 화면이 어두워지고 영역 선택이 시작됩니다.
3. 녹화할 영역을 드래그합니다. 마우스를 놓는 순간 녹화가 시작됩니다.
4. `Enter`를 누르면 녹화가 끝나고 GIF가 만들어집니다. 녹화는 최대 30초까지 됩니다.
5. 영역을 고르는 중이나 녹화 중에 `Esc`를 누르면 취소되고 아무것도 저장되지 않습니다.

GIF는 `사진\midi` 폴더에 `midi-YYYYMMDD-HHMMSS.gif` 이름으로 저장됩니다. 트레이 아이콘을 오른쪽 클릭하면 나오는 메뉴에서 "저장 폴더 열기"로 바로 열 수 있습니다. 이 메뉴에는 "영역 골라 녹화", "창 열기", "종료"도 있습니다.

### 주요 설정

앱 창에서 바꿀 수 있고, 바꾼 값은 다음 실행 때도 유지됩니다.

| 설정 | 내용 |
|---|---|
| 프레임 수 | 10, 15, 30 fps 중 선택 |
| 영역 정하기 | "드래그해서" 또는 "녹화 틀". 녹화 틀은 크기를 조절할 수 있는 틀을 띄우고 [● 시작]으로 녹화를 시작합니다 |
| 녹화 시작 | 드래그를 마치면 바로 시작하거나, 드래그 뒤 틀로 다듬고 [● 시작]으로 시작 |
| 크기 | "드래그한 영역 그대로" 또는 "가로×세로 지정" |
| 화질 단계 | "자동" 또는 직접 고른 단계 |
| 용량 목표 | 없음, 1, 2, 5, 8, 10MB 이하, 또는 직접 입력. 결과가 목표보다 크면 설정을 낮춰 다시 인코딩합니다 |
| 디더링 강도 | 0에서 1 사이 |
| 녹화 단축키 | 기본값은 `Win+Shift+G`. [기본값] 버튼으로 되돌립니다 |
| Windows를 시작할 때 트레이에서 실행 | 기본은 꺼짐 |

## 알려진 한계

- 원격 데스크톱에서는 녹화할 수 없습니다.
- 넷플릭스 같은 DRM으로 보호된 영상은 화면 캡처가 막혀 있어 검게 찍힙니다. midi는 이를 우회하지 않습니다.

## 삭제

Windows 설정 > 앱에서 midi를 골라 제거합니다.

## 버그 신고

[GitHub Issues](https://github.com/NotoriEarth/midi/issues)에 올려 주세요.

## 라이선스

[MIT](LICENSE)

## 개발

### 시작하기

```bash
git clone https://github.com/NotoriEarth/midi.git
cd midi
```

### 구조

| 경로 | 내용 |
|---|---|
| `crates/gif-core` | 순수 Rust GIF 인코더와 녹화 세션 로직. Windows나 Tauri에 의존하지 않음 |
| `crates/gif-win` | Windows 전용 코드. 지금은 화면 캡처(DXGI Desktop Duplication) |
| `app` | Tauri 2 데스크톱 앱. `gif-core`를 호출하는 명령을 화면에 내놓음 |
| `ui` | 앱 화면(HTML, CSS, JavaScript). 빌드 도구 없이 그대로 앱에 포함됨 |
| `docs` | 설계 문서와 결정 기록 |

### 실행

Rust(stable)와 WebView2 런타임이 필요합니다.

```bash
cargo run -p midi-app      # 앱 창 띄우기
cargo test --workspace     # 전체 테스트
cargo test -p gif-win -- --ignored   # 실제 화면이 필요한 캡처 테스트 (화면이 잠겨 있으면 실패)
```

### 설치 파일 만들기

```bash
cd app
cargo tauri build          # target/release/bundle/nsis/midi_<버전>_x64-setup.exe
```
