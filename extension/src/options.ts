/**
 * Options page. Single toggle for the generic (unsupported-site) fallback.
 *
 * Enabling requests the optional <all_urls> host permission, then stores
 * `genericEnabled`. Background watches the storage key and (un)registers the
 * dynamic content scripts. Disabling clears the flag and drops the permission.
 */

const checkbox = document.getElementById("generic") as HTMLInputElement;
const status = document.getElementById("status") as HTMLDivElement;

const ALL_URLS = { origins: ["<all_urls>"] };

function setStatus(msg: string): void {
  status.textContent = msg;
}

async function refresh(): Promise<void> {
  const enabled = (await chrome.storage.local.get("genericEnabled")).genericEnabled === true;
  const hasPerm = await chrome.permissions.contains(ALL_URLS).catch(() => false);
  checkbox.checked = enabled && hasPerm;
  if (enabled && !hasPerm) {
    setStatus("Permission was revoked — re-enable to grant access again.");
    // Storage says on but permission gone: normalize to off.
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
      setStatus("Permission denied — the generic fallback stays off.");
      return;
    }
    await chrome.storage.local.set({ genericEnabled: true });
    setStatus("Active on unsupported sites. Refresh open media tabs to pick it up.");
  } else {
    await chrome.storage.local.set({ genericEnabled: false });
    await chrome.permissions.remove(ALL_URLS).catch(() => undefined);
    setStatus("Disabled. Open tabs keep running until refreshed.");
  }
});

void refresh();
