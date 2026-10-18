/** 两个面板共用原生 dialog：Esc / 关闭按钮关闭，点击遮罩不关闭。 */
export function createModal(className: string, titleText: string, closeLabel: string) {
  const dialog = document.createElement("dialog");
  dialog.className = `modal ${className}`;
  dialog.setAttribute("closedby", "closerequest");
  const header = document.createElement("div");
  header.className = "modal-head";
  const title = document.createElement("h2");
  title.className = "modal-title";
  const close = document.createElement("button");
  close.className = "modal-close";
  close.type = "button";
  close.textContent = "✕";
  close.addEventListener("click", () => dialog.close());
  header.append(title, close);
  dialog.appendChild(header);

  function setLabels(titleText: string, closeLabel: string) {
    title.textContent = titleText;
    dialog.setAttribute("aria-label", titleText);
    close.title = closeLabel;
    close.setAttribute("aria-label", closeLabel);
  }
  setLabels(titleText, closeLabel);
  return { dialog, header, setLabels };
}
