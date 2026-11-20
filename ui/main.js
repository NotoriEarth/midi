const { invoke } = window.__TAURI__.core;

const form = document.querySelector("#settings");
const fps = document.querySelector("#fps");
const colors = document.querySelector("#colors");
const dither = document.querySelector("#dither");
const make = document.querySelector("#make");
const status = document.querySelector("#status");
const preview = document.querySelector("#preview");

function showValue(input, output) {
  const show = () => (output.textContent = input.value);
  input.addEventListener("input", show);
  show();
}

showValue(colors, document.querySelector("#colors-value"));
showValue(dither, document.querySelector("#dither-value"));

form.addEventListener("submit", async (event) => {
  event.preventDefault();
  make.disabled = true;
  status.textContent = "만드는 중…";
  const started = performance.now();

  try {
    const bytes = await invoke("make_sample_gif", {
      fps: Number(fps.value),
      colors: Number(colors.value),
      dither: Number(dither.value),
    });
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
});
