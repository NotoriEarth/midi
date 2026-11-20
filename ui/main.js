const { invoke } = window.__TAURI__.core;

const form = document.querySelector("#settings");
const fps = document.querySelector("#fps");
const colors = document.querySelector("#colors");
const dither = document.querySelector("#dither");
const seconds = document.querySelector("#seconds");
const make = document.querySelector("#make");
const record = document.querySelector("#record");
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

async function showGif(command, args, busyText) {
  make.disabled = record.disabled = true;
  status.textContent = busyText;
  const started = performance.now();

  try {
    const bytes = await invoke(command, args);
    const gif = new Blob([bytes], { type: "image/gif" });
    URL.revokeObjectURL(preview.src);
    preview.src = URL.createObjectURL(gif);
    preview.hidden = false;
    const elapsed = Math.round(performance.now() - started);
    status.textContent = `${gif.size.toLocaleString()}바이트, ${elapsed}ms`;
  } catch (error) {
    status.textContent = `실패: ${error}`;
  } finally {
    make.disabled = record.disabled = false;
  }
}

form.addEventListener("submit", (event) => {
  event.preventDefault();
  showGif("make_sample_gif", settings(), "만드는 중…");
});

record.addEventListener("click", () => {
  const length = Number(seconds.value);
  showGif("record_screen_gif", { ...settings(), seconds: length }, `${length}초 동안 녹화하는 중…`);
});
