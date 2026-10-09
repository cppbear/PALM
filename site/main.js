const controls = document.querySelector("#path-controls");
controls.hidden = false;
controls.addEventListener("change", (event) => {
  const positive = event.target.value === "positive";
  document
    .querySelector("#positive-line")
    .classList.toggle("selected", positive);
  document
    .querySelector("#nonpositive-line")
    .classList.toggle("selected", !positive);
  document.querySelector("#path-condition").textContent = positive
    ? "value > 0"
    : "value <= 0";
  document.querySelector("#path-assertion").textContent = positive
    ? "assert_eq!(classify(2), 1);"
    : "assert_eq!(classify(-1), 0);";
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
