# midi

midi 프로젝트입니다.

## 시작하기

```bash
git clone https://github.com/NotoriEarth/midi.git
cd midi
```

## 구조

| 경로 | 내용 |
|---|---|
| `crates/gif-core` | 순수 Rust GIF 인코더와 녹화 세션 로직. Windows나 Tauri에 의존하지 않음 |
| `crates/gif-win` | Windows 전용 코드. 지금은 화면 캡처(DXGI Desktop Duplication) |
| `app` | Tauri 2 데스크톱 앱. `gif-core`를 호출하는 명령을 화면에 내놓음 |
| `ui` | 앱 화면(HTML, CSS, JavaScript). 빌드 도구 없이 그대로 앱에 포함됨 |
| `docs` | 설계 문서와 결정 기록 |

## 실행

Rust(stable)와 WebView2 런타임이 필요합니다.

```bash
cargo run -p midi-app      # 앱 창 띄우기
cargo test --workspace     # 전체 테스트
cargo test -p gif-win -- --ignored   # 실제 화면이 필요한 캡처 테스트 (화면이 잠겨 있으면 실패)
```

## 라이선스

추후 추가 예정
