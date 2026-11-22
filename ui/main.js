const { invoke } = window.__TAURI__.core;
const { listen } = window.__TAURI__.event;

const form = document.querySelector("#settings");
const fps = document.querySelector("#fps");
const colors = document.querySelector("#colors");
const dither = document.querySelector("#dither");
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
  await showGif("record_screen_gif", settings(), "녹화할 영역을 드래그하세요. Esc나 오른쪽 클릭으로 취소합니다.");
  record.disabled = false;
  showRecording(false);
});

listen("recording-started", ({ payload: [width, height] }) => {
  record.disabled = false;
  showRecording(true);
  status.textContent = `${width}×${height} 녹화하는 중… [끝]을 누르면 GIF로 만듭니다.`;
});

cancel.addEventListener("click", () => invoke("cancel_recording"));

window.addEventListener("keydown", (event) => {
  if (recording && event.key === "Escape") invoke("cancel_recording");
});
