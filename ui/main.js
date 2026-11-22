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
let limit = { milliseconds: 0, by: "time" };
let countdown = 0;

function seconds(milliseconds) {
  return `${Number((milliseconds / 1000).toFixed(1))}초`;
}

function whyItEnded() {
  return limit.by === "time"
    ? `최대 길이 ${seconds(limit.milliseconds)}에 닿아 녹화를 끝냈습니다.`
    : `이 크기와 fps에서는 메모리 한도 때문에 ${seconds(limit.milliseconds)}까지만 녹화할 수 있어 거기서 끝냈습니다. ` +
        "크기를 줄이거나 fps를 낮추면 더 길게 녹화할 수 있습니다.";
}

function describeLevel(number) {
  const [colors, tolerance] = levels[number - 1];
  return `${number}단계 (${colors}색, 열화 ${tolerance})`;
}

function sizeIsFixed() {
  return sizeMode.value === "fit";
}

function levelIsFixed() {
  return level.value !== "";
}

function howTheLimitIsMet() {
  if (sizeIsFixed() && levelIsFixed()) {
    return "크기와 화질 단계를 둘 다 정해서 조정할 것이 없습니다. 둘 중 하나를 자동으로 두면 쓸 수 있습니다.";
  }
  if (sizeIsFixed()) return "크기는 그대로 두고, 화질 단계를 낮춰서 맞춥니다.";
  if (levelIsFixed()) return "화질 단계는 그대로 두고, 크기를 원래의 25%까지 줄여서 맞춥니다.";
  return "화질 단계를 먼저 낮추고, 6단계로도 넘으면 크기를 원래의 25%까지 줄여서 맞춥니다.";
}

function showChoices() {
  const nothingToAdjust = sizeIsFixed() && levelIsFixed();
  sizeBox.hidden = !sizeIsFixed();
  maxSize.disabled = nothingToAdjust;
  maxSizeBox.hidden = nothingToAdjust || maxSize.value !== "custom";
  maxSizeHint.textContent = howTheLimitIsMet();
}

function maxBytes() {
  if ((sizeIsFixed() && levelIsFixed()) || maxSize.value === "") return null;
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
  return sizeIsFixed() ? [Number(sizeWidth.value), Number(sizeHeight.value)] : null;
}

function settings() {
  return { fps: Number(fps.value), quality: quality(), fitWithin: fitWithin() };
}

function showLimit(bytes) {
  if (bytes === null) {
    maxSize.value = "";
    return;
  }
  const megabytes = String(Number((bytes / BYTES_PER_MB).toFixed(2)));
  const listed = [...maxSize.options].some((option) => option.value === megabytes);
  maxSize.value = listed ? megabytes : "custom";
  if (!listed) maxSizeCustom.value = megabytes;
}

function showSettings(saved) {
  fps.value = String(saved.fps);
  sizeMode.value = saved.fitWithin ? "fit" : "original";
  if (saved.fitWithin) [sizeWidth.value, sizeHeight.value] = saved.fitWithin;
  level.value = saved.quality.level === null ? "" : String(saved.quality.level);
  showLimit(saved.quality.maxBytes);
  dither.value = String(saved.quality.dither);
  ditherValue.textContent = dither.value;
  showChoices();
}

function showRecording(now) {
  recording = now;
  clearInterval(countdown);
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
    clearInterval(countdown);
    status.textContent = "GIF로 만드는 중…";
    invoke("stop_recording");
    return;
  }
  record.disabled = true;
  await showGif("record_screen_gif", { settings: settings() }, "녹화할 영역을 드래그하세요. Esc나 오른쪽 클릭으로 취소합니다.");
  record.disabled = false;
  showRecording(false);
});

cancel.addEventListener("click", () => invoke("cancel_recording"));

showSaved.addEventListener("click", () => {
  invoke("show_saved_gif").catch((problem) => (savedPath.textContent = problem));
});

listen("recording-started", ({ payload: [[width, height], [outWidth, outHeight], milliseconds, by] }) => {
  const size =
    width === outWidth && height === outHeight
      ? `${width}×${height}`
      : `${width}×${height} 영역을 ${outWidth}×${outHeight} 크기로`;
  limit = { milliseconds, by };
  const most = by === "memory" ? ` (이 크기와 fps에서는 최대 ${seconds(milliseconds)})` : "";
  const started = performance.now();
  const showTimeLeft = () => {
    const left = milliseconds - (performance.now() - started);
    if (left <= 0) {
      clearInterval(countdown);
      status.textContent = "GIF로 만드는 중…";
      return;
    }
    status.textContent =
      `${size} 녹화하는 중… 남은 시간 ${Math.ceil(left / 1000)}초${most}. ` +
      "Enter나 [끝]으로 마치고, Esc나 [✕]로 취소합니다.";
  };
  record.disabled = false;
  showRecording(true);
  showTimeLeft();
  countdown = setInterval(showTimeLeft, 200);
});

listen("gif-encoded", ({ payload: [usedLevel, withinLimit, [width, height], [finalWidth, finalHeight], endedAtLimit] }) => {
  const shrunk = width !== finalWidth || height !== finalHeight;
  const size = shrunk
    ? `크기 ${width}×${height} → ${finalWidth}×${finalHeight}`
    : `크기 ${finalWidth}×${finalHeight}`;
  const made = `${size}, 화질 ${describeLevel(usedLevel)}`;
  encoded.hidden = false;
  if (withinLimit === null) {
    encoded.textContent = made;
  } else if (withinLimit) {
    encoded.textContent = `용량 목표에 맞춤: ${made}`;
  } else {
    const advice = sizeIsFixed() ? "크기를 줄이거나 더 짧게 녹화하세요." : "더 짧게 녹화하세요.";
    encoded.textContent = `용량 목표보다 큽니다. 조정할 수 있는 끝까지 낮췄습니다: ${made}. ${advice}`;
  }
  if (endedAtLimit) encoded.textContent += `${encoded.textContent.endsWith(".") ? "" : "."} ${whyItEnded()}`;
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
form.addEventListener("change", () => {
  invoke("remember_settings", { settings: settings() }).catch((problem) => (status.textContent = problem));
});
showChoices();

Promise.all([invoke("quality_levels"), invoke("saved_settings")]).then(([fromRust, saved]) => {
  levels = fromRust;
  levels.forEach((_, index) => level.add(new Option(describeLevel(index + 1), String(index + 1))));
  showSettings(saved);
});

invoke("begin_hotkey").then(
  (keys) => (hotkey.textContent = `${keys}: 영역 선택 시작 · Enter: 녹화 끝 · Esc: 취소`),
  (problem) => (hotkey.textContent = problem),
);
