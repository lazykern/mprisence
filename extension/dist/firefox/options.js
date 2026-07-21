// src/options.ts
var checkbox = document.getElementById("generic");
var status = document.getElementById("status");
var ALL_URLS = { origins: ["<all_urls>"] };
function setStatus(msg) {
  status.textContent = msg;
}
async function refresh() {
  const enabled = (await chrome.storage.local.get("genericEnabled")).genericEnabled === true;
  const hasPerm = await chrome.permissions.contains(ALL_URLS).catch(() => false);
  checkbox.checked = enabled && hasPerm;
  if (enabled && !hasPerm) {
    setStatus("Permission was revoked \u2014 re-enable to grant access again.");
    await chrome.storage.local.set({ genericEnabled: false });
  } else {
    setStatus(checkbox.checked ? "Active on unsupported sites." : "");
  }
}
checkbox.addEventListener("change", async () => {
  if (checkbox.checked) {
    const granted = await chrome.permissions.request(ALL_URLS).catch(() => false);
    if (!granted) {
      checkbox.checked = false;
      setStatus("Permission denied \u2014 the generic fallback stays off.");
      return;
    }
    await chrome.storage.local.set({ genericEnabled: true });
    setStatus("Active on unsupported sites. Refresh open media tabs to pick it up.");
  } else {
    await chrome.storage.local.set({ genericEnabled: false });
    await chrome.permissions.remove(ALL_URLS).catch(() => void 0);
    setStatus("Disabled. Open tabs keep running until refreshed.");
  }
});
void refresh();
//# sourceMappingURL=options.js.map
