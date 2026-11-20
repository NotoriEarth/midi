# ezgif.com 분석과 스택 재검토

작성: 2026-11-20. 공개 페이지(첫 화면, 메뉴, 도구별 옵션 폼, 업데이트 공지)와, 그 페이지가 불러오는 클라이언트 스크립트·WebAssembly 파일을 내려받아 읽었습니다. 파일을 실제로 올려 보지는 않았습니다. 서버 코드는 볼 수 없으므로 서버 쪽 구현은 사이트가 스스로 밝힌 내용에 한합니다.

**전제 변경(사용자, 2026-11-20)**: 최종 목표는 ezgif 같은 범용 GIF/영상 편집 도구이고, 화면 영역 캡처([gif-capture-design.md](gif-capture-design.md))는 그 안의 추가 기능입니다.

## 1. 사이트 구조

- **도구 하나 = 페이지 하나.** 파일을 올리고 → 옵션 폼을 채우고 → 결과를 미리 본 뒤 → 결과를 바로 다음 도구로 넘깁니다. 도구는 100개가 넘습니다.
- **미리보기는 브라우저가 합니다.** GIF, APNG, WebP, AVIF 애니메이션과 영상을 브라우저가 직접 재생합니다.
- **두 가지 처리 방식.** 서버 처리(최대 200MB, 1시간 뒤 삭제)와, 2026년 10월부터 넣기 시작한 "로컬 처리"(브라우저 안에서 WebAssembly로 처리, 업로드 없음)입니다. 한 페이지에 폼이 둘 있습니다. 서버용 `#upload-form`은 같은 URL로 multipart POST를 보내고(파일 필드 `new-image`, URL 입력 `new-image-url`), 로컬용 `#client-video-to-gif-form`은 아무것도 전송하지 않습니다.
- **서버 응답.** nginx, HTTP/3, 페이지는 6시간 공개 캐시, 쿠키 없음. 스크립트와 wasm은 내용 해시 이름(`/js/fc56b8ef.wasm`)으로 내려 주고, 페이지 안의 `window.ezgifAssets` 표가 논리 이름과 연결합니다.

## 2. 기능 목록

| 분류 | 도구 |
|---|---|
| 만들기 | GIF Maker(이미지 여러 장 → 애니메이션, 프레임별 위치·정렬), Video to GIF, 화면 녹화, WebP/APNG/AVIF/JXL/MNG Maker |
| 변형 | 크기 조절, 자르기, 회전, 역재생, 속도, 반복 횟수, 구간 자르기, 길이 늘리기, 확대 |
| 최적화 | 손실 압축(lossy LZW), 색상 수 줄이기, n번째 프레임 제거, 중복 프레임 제거, 투명 최적화, 목표 용량 압축, 일괄 처리, GIF 복구 |
| 효과 | 색조·채도·밝기·대비, 흑백·세피아·반전, 배경색 교체·투명화, 뒤집기, 페이드 인/아웃, 블러·샤픈, 디더링(ordered 11종, Floyd–Steinberg, Riemersma), 유화·엠보스·포스터화·노이즈·소용돌이, 초 카운터 |
| 합성 | 텍스트 추가(사용자 글꼴), 흰 상자 자막, 이미지 겹치기, GIF 이어 붙이기, 둥근 모서리, 반사, 검열(모자이크), 배경 제거 |
| 분해 | 프레임 분리, 프레임 편집 |
| 영상 | 자르기, 크롭, 회전, 역재생, 속도, 화면비, 자막, 압축, 합치기, 멀티트랙 타임라인 편집기, 소리 추출·제거 |
| 변환 | GIF ↔ MP4/WebM/MOV, WebP/APNG/AVIF/JXL/SVG/HEIC/RAW/BMP/ICO/Lottie/Aseprite, PDF 도구 |
| 분석 | GIF 분석기, 메타데이터 보기 |

## 3. 내부 구현

### 3.1 사이트가 밝힌 것

- **GIF 최적화**: Gifsicle과 그 손실 LZW 인코더(GPL).
- **영상 디코딩**: FFmpeg. 로컬 처리 모드에서는 브라우저용 FFmpeg WebAssembly로 대체.
- **Video to GIF 인코더(`ezgif-hq`, 2026-10 도입)**: 지각 기반 색상 양자화 + K-Means 보정, 복잡한 프레임에만 프레임별 팔레트, 안 변한 배경 픽셀 재사용. 서버와 브라우저(WebAssembly) 양쪽에서 같은 인코더를 돌립니다.
- **효과 옵션 이름**(fuzz, coalesce, Riemersma, swirl, implode)은 ImageMagick의 용어와 같습니다. ImageMagick을 쓴다고 사이트가 직접 밝힌 것은 아니고, 옵션 이름에서 추정한 것입니다.

### 3.2 클라이언트 코드에서 확인한 것 (로컬 처리 모드)

| 항목 | 확인한 내용 | 근거 |
|---|---|---|
| 프레임 추출 | `<video>`를 시간별로 seek한 뒤 canvas `drawImage` → `getImageData`로 RGBA를 얻음 | `/js/4fe4690b.js` |
| 디코딩 예비 경로 | `<video>`가 재생하지 못하는 형식(AVI, MKV, WMV, FLV 등)만 FFmpeg wasm을 그때 불러옴. 9.1MB, 전송 2.8MB | `/js-src/ffmpeg-fallback.js` 주석 |
| GIF 인코더 | C++을 Emscripten으로 빌드한 자체 모듈. C API는 `vtg_create / alloc_bytes / add_frame / encode / get_output_data / get_output_size / free_bytes / destroy` | 로더 `video-to-gif-hq.js`, wasm 안의 libc++abi 문자열 |
| 인코더 크기 | `video-to-gif-hq.wasm` 57KB, 기본 `video-to-gif.wasm` 46KB. 변형은 4종(기본, kmeans, hq, tuned) | 내려받은 파일 |
| 다른 형식 인코더 | WebP(`vtw`), APNG(`vta`), AVIF(`vtav`)도 접두어만 다른 같은 C API. 공용 실행기 하나가 전부 돌림 | `/js-src/client-wasm-encoder.js` 주석 |
| 실행 방식 | Web Worker에서 인코딩. 처리 대기 프레임을 4개로 제한하고, Worker를 못 쓰면 메인 스레드로 전환 | 같은 파일(`MAX_PENDING = 4`) |
| GIF 후처리 | gifsicle wasm을 Worker로 실행 | `/js/gifsicle-worker.js` |
| 기타 라이브러리 | apngasm, jSquash(AVIF, JXL, oxipng), pdf.js, pdf-lib, JSZip, Sortable | 스크립트 참조 |
| 글꼴 | 시스템 글꼴은 `queryLocalFonts`. 불러온 글꼴을 브라우저에 저장한다는 공지와 IndexedDB 사용이 함께 있음(연결은 추정) | 스크립트 검색, 2026-10-29 공지 |

- **쓰지 않는 것**: WebCodecs(`VideoDecoder`, `VideoEncoder`), `SharedArrayBuffer`(멀티스레드 wasm), WebGL은 내려받은 스크립트 69개(사이트 스크립트 62개, 로더·Worker 7개) 어디에도 없습니다. 사이트의 스크립트 전부를 받은 것은 아닙니다.
- **wasm 안의 이름은 축약돼 있습니다**(가져오기 `a.a`, 내보내기 `f`~`w`). 위 함수 이름은 로더 스크립트에서 읽은 것이고, 인코더 내부 알고리즘은 디컴파일하지 않았습니다.

## 4. 스택 선택에 주는 의미

1. **코드의 대부분이 "옵션 폼 + 미리보기" 화면입니다.** 캡처 기능만 볼 때는 UI가 Win32 창 몇 개뿐이었지만, 이제는 도구 수십 개의 화면이 일의 중심이 됩니다.
2. **미리보기는 웹 엔진이 가장 쉽습니다.** WebView2(Chromium)는 GIF, APNG, WebP, AVIF 애니메이션과 영상을 그냥 재생합니다. WPF는 애니메이션 GIF조차 기본 지원이 없어 형식마다 라이브러리를 붙여야 합니다.
3. **처리 엔진은 FFmpeg + 자체 인코더 조합이 검증된 길입니다.** 영상 디코딩과 필터는 FFmpeg에 맡기고, GIF 인코더만 직접 만듭니다. ezgif도 같은 구성입니다.
4. **인코더 설계가 ezgif-hq와 거의 일치합니다.** 우리 설계(전역 팔레트, 변경 영역만 디더링, 프레임 차분)에 "복잡한 프레임만 프레임별 팔레트"와 "K-Means 보정"을 더하면 됩니다.
5. **Rust로 짠 처리 코어는 WebAssembly로도 빌드됩니다.** 나중에 웹 버전을 내고 싶어지면 ezgif의 로컬 처리 모드와 같은 방식으로 재사용할 수 있습니다. C# 코어는 이 길이 훨씬 무겁습니다.
6. **인코더 경계는 ezgif의 C API 모양을 그대로 따르면 됩니다.** `create(옵션) → add_frame(RGBA, 지연 시간) 반복 → encode → 결과 바이트`. 57KB짜리 모듈로 충분하다는 것이 확인됐고, 형식마다 같은 모양을 쓰면 실행기 하나로 전부 돌릴 수 있습니다.
7. **흔한 영상은 웹 엔진만으로 프레임을 뽑을 수 있습니다.** ezgif는 `<video>` + canvas를 먼저 쓰고 FFmpeg는 실패할 때만 부릅니다. 데스크톱에서는 FFmpeg를 바로 호출할 수 있으니 필수는 아니지만, 미리보기 화면에서 구간과 크롭을 고를 때는 같은 방식이 가장 간단합니다.

## 5. 재추천: Tauri 2 (Rust 코어 + 웹 UI)

[stack-proposal.md](stack-proposal.md)의 C#/WPF 추천은 "이 기능의 UI는 전부 Win32 창이라 웹뷰의 이점이 없다"가 근거였습니다. 전제가 바뀌어 그 근거가 사라졌으므로 추천을 바꿉니다.

| | Tauri 2 (추천) | C# + WPF |
|---|---|---|
| 도구 수십 개의 폼 화면 | 웹 UI, 빠름 | XAML, 빠름 |
| 애니메이션·영상 미리보기 | 웹 엔진이 기본 재생 | 형식마다 라이브러리 필요 |
| 타임라인, 프레임 편집, 텍스트 배치 | Canvas와 웹 생태계 | 직접 구현 |
| 화면 캡처(DXGI, 오버레이, 핫키) | Rust `windows` crate, Win32 창 | CsWin32 + Vortice, Win32 창 |
| GIF 인코더 속도 | 빠름 | 1.3~2배 느림(추정) |
| 나중에 웹 버전 | 코어를 WebAssembly로 재사용 | 어려움 |
| 배포 | 5~15MB + WebView2 + FFmpeg | .NET 런타임 또는 60~100MB, + FFmpeg |
| 부담 | 언어 두 개(Rust + TypeScript), Rust 학습 | 미리보기·편집 UI를 직접 구현 |

**구조**
- `gif-core`(순수 Rust): 인코더, 프레임 연산(크기·자르기·속도·역재생·효과), 세션 상태 머신. WebAssembly로도 빌드 가능하게 OS 의존 없이 둡니다.
- `gif-win`: 화면 캡처, 오버레이, 핫키, 클립보드. Win32 호출은 여기에만.
- `media`: FFmpeg 호출(영상 디코딩, 영상 편집, 형식 변환).
- `app`(Tauri) + `ui`(TypeScript): 도구 하나가 화면 하나 + Rust 명령 하나. ezgif처럼 결과를 다음 도구로 넘기는 흐름.

stack-proposal.md §2의 공통 구조(Win32 창, 직접 등록하는 핫키, 순수 함수 상태 머신, 직접 구현 인코더)는 그대로 유효합니다. candidate-2의 핫키 스레드 결함(등록한 스레드에서만 해제 가능)은 핫키를 UI 스레드가 소유하도록 고쳐서 가져갑니다.

## 6. 주의할 점

- **라이선스**: Gifsicle은 GPL-2.0입니다. ezgif는 서버에서만 돌리지만, 데스크톱 앱에 넣어 배포하면 GPL 의무가 생깁니다. 손실 LZW는 직접 구현하는 기존 계획을 유지합니다. FFmpeg는 LGPL 구성으로 빌드해 별도 실행 파일로 둡니다.
- **범위**: ezgif는 10년 넘게 쌓인 도구 모음입니다. 처음에는 Video to GIF, 크기·자르기·속도, 최적화, 텍스트 추가 정도로 좁히는 게 현실적입니다.
- **미확인**: WebView2의 JXL 재생 지원 여부, FFmpeg 최소 빌드 크기, ezgif의 서버 언어와 서버 쪽 도구 구성, 업로드 후 결과 URL 형태(파일을 올려 보지 않음), HQ 인코더 내부 알고리즘(공지 내용만 있음).
