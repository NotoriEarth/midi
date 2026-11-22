const { invoke } = window.__TAURI__.core;
const { listen } = window.__TAURI__.event;

const form = document.querySelector("#settings");
const fps = document.querySelector("#fps");
const dither = document.querySelector("#dither");
const sizeMode = document.querySelector("#size-mode");
const sizeBox = document.querySelector("#size-box");
const sizeWidth = document.querySelector("#size-width");
const sizeHeight = document.querySelector("#size-height");
const level = document.querySelector("#level");
const maxSize = document.querySelector("#max-size");
const maxSizeBox = document.querySelector("#max-size-box");
const maxSizeCustom = document.querySelector("#max-size-custom");
const maxSizeHint = document.querySelector("#max-size-hint");
const make = document.querySelector("#make");
const record = document.querySelector("#record");
const cancel = document.querySelector("#cancel");
const status = document.querySelector("#status");
const encoded = document.querySelector("#encoded");
const preview = document.querySelector("#preview");
const saved = document.querySelector("#saved");
const savedPath = document.querySelector("#saved-path");
const showSaved = document.querySelector("#show-saved");
const hotkey = document.querySelector("#hotkey");

const BYTES_PER_MB = 1024 * 1024;
let levels = [];
let recording = false;

function describeLevel(number) {
  const [colors, tolerance] = levels[number - 1];
  return `${number}단계 (${colors}색, 열화 ${tolerance})`;
}

function showChoices() {
  const automatic = level.value === "";
  sizeBox.hidden = sizeMode.value !== "fit";
  maxSize.disabled = !automatic;
  maxSizeBox.hidden = !automatic || maxSize.value !== "custom";
  maxSizeHint.textContent = automatic
    ? "크기는 그대로 두고, 이 용량 안에 들어오는 가장 좋은 화질 단계를 고릅니다."
    : "화질 단계를 직접 골랐기 때문에 쓰이지 않습니다. 화질 단계를 자동으로 두면 쓸 수 있습니다.";
}

function maxBytes() {
  if (level.value !== "" || maxSize.value === "") return null;
  const megabytes = maxSize.value === "custom" ? Number(maxSizeCustom.value) : Number(maxSize.value);
  return Math.round(megabytes * BYTES_PER_MB);
}

function quality() {
  return {
    level: level.value === "" ? null : Number(level.value),
    maxBytes: maxBytes(),
    dither: Number(dither.value),
  };
}

function fitWithin() {
  return sizeMode.value === "fit" ? [Number(sizeWidth.value), Number(sizeHeight.value)] : null;
}

function showRecording(now) {
  recording = now;
  record.textContent = now ? "끝" : "녹화 시작";
  cancel.hidden = !now;
  make.disabled = now;
}

async function showGif(command, args, busyText) {
  make.disabled = true;
  encoded.hidden = true;
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
  showGif("make_sample_gif", { fps: Number(fps.value), quality: quality() }, "만드는 중…");
});

record.addEventListener("click", async () => {
  if (recording) {
    status.textContent = "GIF로 만드는 중…";
    invoke("stop_recording");
    return;
  }
  record.disabled = true;
  const settings = { fps: Number(fps.value), quality: quality(), fitWithin: fitWithin() };
  await showGif("record_screen_gif", { settings }, "녹화할 영역을 드래그하세요. Esc나 오른쪽 클릭으로 취소합니다.");
  record.disabled = false;
  showRecording(false);
});

cancel.addEventListener("click", () => invoke("cancel_recording"));

showSaved.addEventListener("click", () => {
  invoke("show_saved_gif").catch((problem) => (savedPath.textContent = problem));
});

listen("recording-started", ({ payload: [[width, height], [outWidth, outHeight]] }) => {
  const size =
    width === outWidth && height === outHeight
      ? `${width}×${height}`
      : `${width}×${height} 영역을 ${outWidth}×${outHeight} 크기로`;
  record.disabled = false;
  showRecording(true);
  status.textContent = `${size} 녹화하는 중… Enter나 [끝]으로 마치고, Esc나 [✕]로 취소합니다.`;
});

listen("gif-encoded", ({ payload: [usedLevel, withinLimit, [width, height]] }) => {
  encoded.hidden = false;
  if (withinLimit === null) {
    encoded.textContent = `${width}×${height}, 화질 ${describeLevel(usedLevel)}`;
  } else if (withinLimit) {
    encoded.textContent = `${width}×${height}, 용량 목표에 맞춘 화질: ${describeLevel(usedLevel)}`;
  } else {
    encoded.textContent =
      `${width}×${height} 크기에서는 가장 낮은 화질 ${describeLevel(usedLevel)}로도 용량 목표보다 큽니다. ` +
      "크기를 줄이거나 더 짧게 녹화하세요.";
  }
});

listen("gif-saved", ({ payload }) => {
  saved.hidden = false;
  showSaved.hidden = !("Ok" in payload);
  savedPath.textContent = "Ok" in payload ? `저장: ${payload.Ok}` : `저장 실패: ${payload.Err}`;
});

listen("begin-selection", () => {
  if (!recording && !record.disabled) record.click();
});

const ditherValue = document.querySelector("#dither-value");
dither.addEventListener("input", () => (ditherValue.textContent = dither.value));
for (const choice of [sizeMode, level, maxSize]) choice.addEventListener("change", showChoices);
showChoices();

invoke("quality_levels").then((fromRust) => {
  levels = fromRust;
  levels.forEach((_, index) => level.add(new Option(describeLevel(index + 1), String(index + 1))));
});

invoke("begin_hotkey").then(
  (keys) => (hotkey.textContent = `${keys}: 영역 선택 시작 · Enter: 녹화 끝 · Esc: 취소`),
  (problem) => (hotkey.textContent = problem),
);
