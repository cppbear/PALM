const controls = document.querySelector("#path-controls");
const workbench = document.querySelector(".workbench");
const traceButton = document.querySelector("#trace-button");
const traceLabel = traceButton.querySelector("span");
const traceStatus = document.querySelector("#trace-status");
const traceSteps = [...document.querySelectorAll("[data-trace-step]")];
const instantTrace = matchMedia(
  "(max-width: 760px), (prefers-reduced-motion: reduce)",
);
let traceTimer = null;

function updateTraceLabel() {
  traceLabel.textContent = instantTrace.matches
    ? "Show path summary"
    : workbench.dataset.trace === "complete"
      ? "Replay path"
      : "Trace this path";
}

function resetTrace() {
  clearTimeout(traceTimer);
  traceTimer = null;
  delete workbench.dataset.trace;
  traceSteps.forEach((step) => step.removeAttribute("aria-current"));
  updateTraceLabel();
  traceStatus.textContent =
    "Visual walkthrough of the fixed example. No code is executed.";
}

function finishTrace() {
  clearTimeout(traceTimer);
  traceTimer = null;
  workbench.dataset.trace = "complete";
  traceSteps.forEach((step) => step.removeAttribute("aria-current"));
  updateTraceLabel();
  const positive = workbench.dataset.active === "positive";
  traceStatus.textContent = positive
    ? "Path illustrated: input 2 → true branch → return 1 → classify(2) == 1."
    : "Path illustrated: input −1 → false branch → return 0 → classify(-1) == 0.";
}

traceButton.addEventListener("click", () => {
  if (traceTimer !== null) {
    resetTrace();
    traceStatus.textContent =
      "Walkthrough stopped. The selected example remains visible.";
    return;
  }
  if (instantTrace.matches) {
    finishTrace();
    return;
  }
  const positive = workbench.dataset.active === "positive";
  const messages = [
    `1 of 3: inspect the source with input ${positive ? "2" : "−1"}.`,
    `2 of 3: follow the ${positive ? "true" : "false"} branch to return ${positive ? "1" : "0"}.`,
    "3 of 3: inspect the matching assertion from the fixed example.",
  ];
  traceLabel.textContent = "Stop trace";
  function showStep(index) {
    workbench.dataset.trace = traceSteps[index].dataset.traceStep;
    traceSteps.forEach((step, i) => {
      if (i === index) step.setAttribute("aria-current", "step");
      else step.removeAttribute("aria-current");
    });
    traceStatus.textContent = messages[index];
    traceTimer = setTimeout(
      () => (index < 2 ? showStep(index + 1) : finishTrace()),
      1000,
    );
  }
  showStep(0);
});
instantTrace.addEventListener("change", () => {
  if (instantTrace.matches && traceTimer !== null) finishTrace();
  else updateTraceLabel();
});

updateTraceLabel();
controls.hidden = false;
document.querySelector("#trace-controls").hidden = false;
controls.addEventListener("change", (event) => {
  resetTrace();
  const positive = event.target.value === "positive";
  workbench.dataset.active = event.target.value;
  document.querySelector("#condition-value").textContent = String(positive);
  document.querySelector("#path-assertion").textContent = positive
    ? "assert_eq!(\n    classify(2),\n    1\n);"
    : "assert_eq!(\n    classify(-1),\n    0\n);";
  document.querySelector("#return-value").textContent = positive ? "1" : "0";
  const details = document.querySelectorAll("[data-trace-detail]");
  details[0].textContent = positive ? "input 2" : "input −1";
  details[1].textContent = positive ? "true → 1" : "false → 0";
  details[2].textContent = positive ? "classify(2) == 1" : "classify(-1) == 0";
});
const copyButton = document.querySelector("[data-copy]");
copyButton.hidden = false;
copyButton.addEventListener("click", async () => {
  const status = document.querySelector("#copy-status");
  try {
    await navigator.clipboard.writeText(
      document.getElementById(copyButton.dataset.copy).textContent,
    );
    status.textContent = "Commands copied.";
  } catch {
    status.textContent =
      "Copy unavailable. Select the commands above and copy them manually.";
  }
});
