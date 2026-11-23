// Checks that the midi settings page fits a 720x600 window in every state.
// Usage: node tools/check-ui-layout.mjs <cdp-port>   (the app must run with --remote-debugging-port)
const port = process.argv[2];
const WIDTH = 720;
const HEIGHT = 600;
const IDS = `settings fps selection record-control record-control-hint size-mode size-box size-width size-height level
  max-size max-size-box max-size-custom max-size-hint dither-value dither record cancel hotkey hotkey-input
  hotkey-default autostart status encoded saved saved-path show-saved preview`.split(/\s+/);
const WIDE_GIF =
  "data:image/svg+xml," + encodeURIComponent("<svg xmlns='http://www.w3.org/2000/svg' width='637' height='421'><rect width='637' height='421' fill='#5f7cf0'/></svg>");

async function pageSocket() {
  for (let attempt = 0; attempt < 120; attempt++) {
    try {
      const targets = await (await fetch(`http://127.0.0.1:${port}/json`)).json();
      const page = targets.find((target) => target.type === "page" && target.url !== "about:blank");
      if (page) return new WebSocket(page.webSocketDebuggerUrl);
    } catch {}
    await new Promise((resolve) => setTimeout(resolve, 500));
  }
  throw new Error("the app window did not expose a page");
}

const socket = await pageSocket();
await new Promise((resolve, reject) => {
  socket.onopen = resolve;
  socket.onerror = reject;
});
let nextId = 1;
const waiting = new Map();
socket.onmessage = (message) => {
  const reply = JSON.parse(message.data);
  waiting.get(reply.id)?.(reply);
};
function send(method, params = {}) {
  const id = nextId++;
  socket.send(JSON.stringify({ id, method, params }));
  return new Promise((resolve) => waiting.set(id, resolve));
}
async function evaluate(expression) {
  const reply = await send("Runtime.evaluate", { expression, awaitPromise: true, returnByValue: true });
  if (reply.result?.exceptionDetails) throw new Error(JSON.stringify(reply.result.exceptionDetails));
  return reply.result?.result?.value;
}
const readyPage = () =>
  evaluate(`new Promise((resolve) => {
    const check = () => (document.readyState === "complete" && document.querySelector("#level")?.options.length > 1 ? resolve(true) : setTimeout(check, 100));
    check();
  })`);

await send("Emulation.setDeviceMetricsOverride", { width: WIDTH, height: HEIGHT, deviceScaleFactor: 1, mobile: false });

const show = (selector) => `document.querySelector(${JSON.stringify(selector)}).hidden = false;`;
const choose = (selector, value) =>
  `{ const e = document.querySelector(${JSON.stringify(selector)}); e.value = ${JSON.stringify(value)}; e.dispatchEvent(new Event("change", { bubbles: true })); }`;
const STATES = [
  { name: "just opened", change: "", expect: "previewHidden" },
  {
    name: "with a result",
    change:
      `document.querySelector("#preview").src = ${JSON.stringify(WIDE_GIF)};` +
      show("#preview") + show("#saved") + show("#encoded") +
      `document.querySelector("#status").textContent = "1,681,971바이트, 4,700ms";` +
      `document.querySelector("#encoded").textContent = "용량 목표에 맞춤: 크기 640×360 → 384×216, 화질 6단계 (16색, 열화 40)";` +
      `document.querySelector("#saved-path").textContent = "저장: C:\\\\Users\\\\someone\\\\Pictures\\\\midi\\\\midi-20261123-222150.gif";`,
  },
  {
    name: "every conditional field open",
    change: choose("#selection", "frame") + choose("#size-mode", "fit") + choose("#max-size", "custom"),
    expect: "conditionalShown",
  },
  { name: "recording", change: show("#cancel") + `document.querySelector("#record").textContent = "끝";`, expect: "buttonsInside" },
  {
    name: "very long texts",
    change:
      show("#preview") + show("#saved") + show("#encoded") +
      `document.querySelector("#status").textContent = "가".repeat(200);` +
      `document.querySelector("#encoded").textContent = "나".repeat(120);`,
    expect: "resultScrolls",
  },
];

const MEASURE = `(() => {
  const problems = [];
  const root = document.scrollingElement;
  if (root.scrollHeight > ${HEIGHT}) problems.push("scrollHeight " + root.scrollHeight + " > ${HEIGHT}");
  if (root.scrollWidth > ${WIDTH}) problems.push("scrollWidth " + root.scrollWidth + " > ${WIDTH}");
  for (const id of ${JSON.stringify(IDS)}) if (!document.getElementById(id)) problems.push("#" + id + " missing");
  if (document.getElementById("make")) problems.push("#make still present");
  for (const id of ["autostart", "hotkey-input", "hotkey-default"])
    if (document.querySelector("#settings #" + id)) problems.push("#" + id + " is inside the settings form");
  for (const id of ["level", "size-mode", "max-size"]) {
    const e = document.getElementById(id);
    if (e && e.offsetParent && e.scrollWidth > e.clientWidth) problems.push("#" + id + " text is cut off");
  }
  const visible = (selector) => { const e = document.querySelector(selector); return !!e && e.getClientRects().length > 0; };
  const inside = (selector) => { const r = document.querySelector(selector).getBoundingClientRect(); return r.left >= 0 && r.top >= 0 && r.right <= ${WIDTH} && r.bottom <= ${HEIGHT}; };
  return { problems, visible: {
    preview: visible("#preview"), saved: visible("#saved"), encoded: visible("#encoded"),
    sizeBox: visible("#size-box"), maxSizeBox: visible("#max-size-box"), recordHint: visible("#record-control-hint"),
    empty: [...document.querySelectorAll("body *")].some((e) => e.getClientRects().length && /아직 녹화한 GIF가 없습니다/.test(e.textContent) && e.children.length === 0),
  }, buttonsInside: inside("#record") && inside("#cancel"),
  resultOverflowY: (() => { const box = document.querySelector("#status")?.parentElement; return box ? getComputedStyle(box).overflowY : ""; })() };
})()`;

let failed = false;
for (const state of STATES) {
  await send("Page.reload");
  await new Promise((resolve) => setTimeout(resolve, 300));
  await readyPage();
  if (state.change) await evaluate(`(() => { ${state.change} })()`);
  await new Promise((resolve) => setTimeout(resolve, 150));
  const seen = await evaluate(MEASURE);
  const problems = [...seen.problems];
  if (state.expect === "previewHidden") {
    if (seen.visible.preview || seen.visible.saved || seen.visible.encoded) problems.push("result lines show with no result");
    if (!seen.visible.empty) problems.push("no empty-state message where the preview goes");
  }
  if (state.expect === "conditionalShown" && !(seen.visible.sizeBox && seen.visible.maxSizeBox && seen.visible.recordHint))
    problems.push("conditional fields did not all appear: " + JSON.stringify(seen.visible));
  if (state.expect === "buttonsInside" && !seen.buttonsInside) problems.push("record/cancel buttons leave the window");
  if (state.expect === "resultScrolls" && seen.resultOverflowY !== "auto") problems.push("the result box does not scroll on its own (overflow-y " + seen.resultOverflowY + ")");
  if (problems.length) {
    failed = true;
    console.log(`FAIL ${state.name}: ${problems.join("; ")}`);
  } else {
    console.log(`ok   ${state.name}`);
  }
}
socket.close();
if (failed) process.exit(1);
console.log("layout ok");
