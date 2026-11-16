// Connect screen: pick an ORYN server, check it, then hand the window over to its web UI.
(function () {
  const invoke = window.__TAURI__?.core?.invoke;
  const form = document.getElementById("connect-form");
  const input = document.getElementById("server-url");
  const connectButton = document.getElementById("connect");
  const openAnyway = document.getElementById("open-anyway");
  const status = document.getElementById("status");

  function setStatus(message, isError) {
    status.textContent = message;
    status.classList.toggle("error", Boolean(isError));
  }

  function setBusy(busy) {
    connectButton.disabled = busy;
    openAnyway.disabled = busy;
    input.disabled = busy;
  }

  async function connect(force) {
    const url = input.value.trim();
    openAnyway.hidden = true;
    setBusy(true);
    setStatus(force ? "Opening " + url + "…" : "Connecting to " + url + "…", false);
    try {
      await invoke("connect", { url, force });
      // On success the window navigates away; nothing more to do here.
    } catch (error) {
      setBusy(false);
      setStatus(error?.message ?? String(error), true);
      // A server that answers oddly (or not yet) may still be worth opening.
      openAnyway.hidden = error?.kind === "invalid";
      input.focus();
    }
  }

  form.addEventListener("submit", (event) => {
    event.preventDefault();
    connect(false);
  });
  openAnyway.addEventListener("click", () => connect(true));

  async function start() {
    if (!invoke) {
      setStatus("This page only works inside the ORYN app.", true);
      return;
    }
    input.value = await invoke("get_server_url");
    // Launch connects straight away; ⌘, ("Server…") opens this screen with ?manual=1.
    const manual = new URLSearchParams(window.location.search).has("manual");
    if (manual) {
      input.focus();
      input.select();
    } else {
      connect(false);
    }
  }

  start();
})();
