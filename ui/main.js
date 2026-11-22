const { invoke } = window.__TAURI__.core;
const { listen } = window.__TAURI__.event;

const form = document.querySelector("#settings");
const fps = document.querySelector("#fps");
const colors = document.querySelector("#colors");
const dither = document.querySelector("#dither");
const scale = document.querySelector("#scale");
const tolerance = document.querySelector("#tolerance");
const make = document.querySelector("#make");
const record = document.querySelector("#record");
const cancel = document.querySelector("#cancel");
const status = document.querySelector("#status");
const preview = document.querySelector("#preview");

function showValue(input, output) {
  const show = () => (output.textContent = input.value);
  input.addEventListener("input", show);
  show();
}

showValue(colors, document.querySelector("#colors-value"));
showValue(dither, document.querySelector("#dither-value"));
showValue(tolerance, document.querySelector("#tolerance-value"));

function settings() {
  return { fps: Number(fps.value), colors: Number(colors.value), dither: Number(dither.value) };
}

let recording = false;

function showRecording(now) {
  recording = now;
  record.textContent = now ? "끝" : "녹화 시작";
  cancel.hidden = !now;
  make.disabled = now;
}

async function showGif(command, args, busyText) {
  make.disabled = true;
  status.textContent = busyText;
  const started = performance.now();

  try {
    const bytes = await invoke(command, args);
    if (bytes.byteLength === 0) {
      status.textContent = "녹화를 취소했습니다.";
      return;
    }
    const gif = new Blob([bytes], { type: "image/gif" });
    URL.revokeObjectURL(preview.src);
    preview.src = URL.createObjectURL(gif);
    preview.hidden = false;
    const elapsed = Math.round(performance.now() - started);
    status.textContent = `${gif.size.toLocaleString()}바이트, ${elapsed}ms`;
  } catch (error) {
    status.textContent = `실패: ${error}`;
  } finally {
    make.disabled = false;
  }
}

form.addEventListener("submit", (event) => {
  event.preventDefault();
  showGif("make_sample_gif", settings(), "만드는 중…");
});

record.addEventListener("click", async () => {
  if (recording) {
    status.textContent = "GIF로 만드는 중…";
    invoke("stop_recording");
    return;
  }
  record.disabled = true;
  await showGif("record_screen_gif", { settings: { ...settings(), tolerance: Number(tolerance.value), scale: Number(scale.value) } }, "녹화할 영역을 드래그하세요. Esc나 오른쪽 클릭으로 취소합니다.");
  record.disabled = false;
  showRecording(false);
});

listen("recording-started", ({ payload: [[width, height], [outWidth, outHeight]] }) => {
  const size = width === outWidth ? `${width}×${height}` : `${width}×${height} 영역을 ${outWidth}×${outHeight} 크기로`;
  record.disabled = false;
  showRecording(true);
  status.textContent = `${size} 녹화하는 중… Enter나 [끝]으로 마치고, Esc나 [✕]로 취소합니다.`;
});

const saved = document.querySelector("#saved");
const savedPath = document.querySelector("#saved-path");
const showSaved = document.querySelector("#show-saved");

listen("gif-saved", ({ payload }) => {
  saved.hidden = false;
  showSaved.hidden = !("Ok" in payload);
  savedPath.textContent = "Ok" in payload ? `저장: ${payload.Ok}` : `저장 실패: ${payload.Err}`;
});

showSaved.addEventListener("click", () => {
  invoke("show_saved_gif").catch((problem) => (savedPath.textContent = problem));
});

cancel.addEventListener("click", () => invoke("cancel_recording"));

listen("begin-selection", () => {
  if (!recording && !record.disabled) record.click();
});

const hotkey = document.querySelector("#hotkey");
invoke("begin_hotkey").then(
  (keys) => (hotkey.textContent = `${keys}: 영역 선택 시작 · Enter: 녹화 끝 · Esc: 취소`),
  (problem) => (hotkey.textContent = problem),
);
