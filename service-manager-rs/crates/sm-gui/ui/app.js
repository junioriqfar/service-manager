const invoke = (command, args) => window.__TAURI__.core.invoke(command, args);
const listen = (event, handler) => window.__TAURI__.event.listen(event, handler);

let services = [];
let selected = null;
let admin = false;
let batchRunning = false;
let refreshing = false;
let refreshTimer = null;
let logTimer = null;

const $ = (id) => document.getElementById(id);

function escapeHtml(value) {
  return String(value ?? "")
    .replaceAll("&", "&amp;")
    .replaceAll("<", "&lt;")
    .replaceAll(">", "&gt;")
    .replaceAll('"', "&quot;")
    .replaceAll("'", "&#39;");
}

function typeLabel(type) {
  if (type === "apache") return "Apache";
  if (type === "postgresql") return "PostgreSQL";
  return "NSSM";
}

function startupLabel(startup) {
  if (startup === "delayed") return "Auto (Delayed)";
  if (startup === "manual") return "Manual";
  return "Automatic";
}

function restartLabel(service) {
  return service.restartOnCrash ? `Yes (${service.restartDelaySec}s)` : "No";
}

function selectedService() {
  return services.find((service) => service.name === selected) || null;
}

function toast(message, type = "", timeout = 4500) {
  const element = document.createElement("div");
  element.className = `toast ${type}`;
  element.textContent = String(message);
  $("toasts").appendChild(element);
  setTimeout(() => element.remove(), timeout);
}

function setStatus(text) {
  $("status-text").textContent = text;
}

function showModal(id) {
  $(id).classList.remove("hidden");
}

function closeModal(id) {
  $(id).classList.add("hidden");
}

function render() {
  const current = selectedService();
  if (!current) selected = null;

  const body = $("service-body");
  body.innerHTML = services
    .map((service) => {
      const rowClass = service.name === selected ? "selected" : "";
      return `<tr data-name="${escapeHtml(service.name)}" class="${rowClass}">
        <td><span class="status ${escapeHtml(service.state)}">${escapeHtml(service.stateLabel)}</span></td>
        <td title="${escapeHtml(service.name)}">${escapeHtml(service.name)}</td>
        <td title="${escapeHtml(service.displayName)}">${escapeHtml(service.displayName)}</td>
        <td>${escapeHtml(typeLabel(service.serviceType))}</td>
        <td title="${escapeHtml(service.executable)}">${escapeHtml(service.executable)}</td>
        <td>${escapeHtml(startupLabel(service.startup))}</td>
        <td>${escapeHtml(restartLabel(service))}</td>
      </tr>`;
    })
    .join("");

  $("empty-state").classList.toggle("hidden", services.length > 0);
  const installed = services.filter((service) => service.installed).length;
  const running = services.filter((service) => service.state === "running").length;
  $("counts").textContent = `${services.length} registered | ${installed} installed | ${running} running`;
  $("status-counts").textContent = $("counts").textContent;
  updateButtons();
  updateLogHeader();
}

function updateButtons() {
  const service = selectedService();
  const installed = Boolean(service && service.installed);
  const running = Boolean(service && service.state === "running");
  const hasInstalled = services.some((item) => item.installed);
  const busy = batchRunning;

  $("btn-start-all").disabled = !hasInstalled || busy;
  $("btn-stop-all").disabled = !hasInstalled || busy;
  $("btn-restart-all").disabled = !hasInstalled || busy;
  $("btn-log-open").disabled = !service;
  $("btn-log-reload").disabled = !service;
  updateContextMenuState();
}

function updateLogHeader() {
  const service = selectedService();
  if (!service) {
    $("log-title").textContent = "Service log: (none selected)";
    $("log-path").textContent = "";
    return;
  }
  $("log-title").textContent = `Service log: ${service.name}`;
}

async function refresh() {
  if (refreshing) return;
  refreshing = true;
  try {
    const next = await invoke("list_services");
    services = Array.isArray(next) ? next : [];
    render();
  } catch (error) {
    toast(error, "error");
  } finally {
    refreshing = false;
  }
}

async function loadLog(force = false) {
  const service = selectedService();
  if (!service) {
    $("log-output").textContent = "";
    return;
  }
  try {
    const result = await invoke("read_log", { name: service.name });
    $("log-path").textContent = result.path;
    const output = $("log-output");
    const nearBottom =
      output.scrollTop + output.clientHeight >= output.scrollHeight - 24;
    if (force || output.textContent !== result.text) {
      output.textContent = result.text;
      if (force || nearBottom) output.scrollTop = output.scrollHeight;
    }
  } catch (error) {
    $("log-output").textContent = String(error);
  }
}

function selectService(name) {
  selected = name;
  render();
  loadLog(true);
}

function fillServiceForm(service) {
  $("f-type").value = service.serviceType;
  $("f-name").value = service.name;
  $("f-display").value = service.displayName;
  $("f-exe").value = service.executable;
  $("f-args").value = service.arguments;
  $("f-work").value = service.workingDirectory;
  $("f-startup").value = service.startup;
  $("f-delay").value = service.restartDelaySec;
  $("f-deps").value = (service.dependencies || []).join(", ");
  $("f-restart").checked = Boolean(service.restartOnCrash);
  $("f-stdout").value = service.stdoutLog;
  $("f-stderr").value = service.stderrLog;
  $("f-apache-root").value = service.apacheRoot;
  $("f-apache-config").value = service.apacheConfig;
  $("f-pg-data").value = service.pgData;
  $("f-pg-account").value = service.pgAccount;
  updateNativeFields();
}

function emptyService() {
  return {
    name: "",
    displayName: "",
    serviceType: "nssm",
    executable: "",
    arguments: "",
    workingDirectory: "",
    apacheRoot: "",
    apacheConfig: "",
    pgData: "",
    pgAccount: "",
    stdoutLog: "",
    stderrLog: "",
    startup: "auto",
    restartOnCrash: true,
    restartDelaySec: 5,
    dependencies: [],
    addedAt: "",
    updatedAt: "",
  };
}

function openServiceModal(edit) {
  if (edit && !selectedService()) {
    toast("Select a service to edit.", "error");
    return;
  }
  const service = edit ? selectedService() : emptyService();
  $("service-modal-title").textContent = edit ? "Edit Service" : "Add Service";
  $("service-hint").textContent = edit
    ? "The service name cannot be changed while editing."
    : "The service can be installed after the config is saved.";
  $("service-error").classList.add("hidden");
  $("f-name").disabled = edit;
  fillServiceForm(service);
  $("service-modal").dataset.edit = edit ? "1" : "0";
  showModal("service-modal");
}

function updateNativeFields() {
  const type = $("f-type").value;
  document.querySelectorAll(".native-field").forEach((element) => {
    element.classList.toggle("hidden", element.dataset.native !== type);
  });
}

function splitDependencies(value) {
  return value
    .split(/[\s,]+/)
    .map((item) => item.trim())
    .filter(Boolean);
}

function collectService() {
  const edit = $("service-modal").dataset.edit === "1";
  const previous = edit ? selectedService() : null;
  const now = new Date().toISOString();
  return {
    name: $("f-name").value.trim(),
    displayName: $("f-display").value.trim(),
    serviceType: $("f-type").value,
    executable: $("f-exe").value.trim(),
    arguments: $("f-args").value.trim(),
    workingDirectory: $("f-work").value.trim(),
    apacheRoot: $("f-apache-root").value.trim(),
    apacheConfig: $("f-apache-config").value.trim(),
    pgData: $("f-pg-data").value.trim(),
    pgAccount: $("f-pg-account").value.trim(),
    stdoutLog: $("f-stdout").value.trim(),
    stderrLog: $("f-stderr").value.trim(),
    startup: $("f-startup").value,
    restartOnCrash: $("f-restart").checked,
    restartDelaySec: Math.max(1, Number.parseInt($("f-delay").value, 10) || 5),
    dependencies: splitDependencies($("f-deps").value),
    addedAt: previous?.addedAt || now,
    updatedAt: now,
  };
}

async function browseExecutable() {
  try {
    const path = await invoke("pick_executable", { current: $("f-exe").value });
    if (!path) return;
    $("f-exe").value = path;
    if (!$("f-work").value) {
      const separator = Math.max(path.lastIndexOf("\\"), path.lastIndexOf("/"));
      if (separator > 0) $("f-work").value = path.slice(0, separator);
    }
    if (!$("f-display").value) {
      const fileName = path.split(/[\\/]/).pop() || "";
      $("f-display").value = fileName.replace(/\.exe$/i, "");
    }
  } catch (error) {
    showServiceError(error);
  }
}

async function browseWorkingDirectory() {
  try {
    const path = await invoke("pick_directory", { current: $("f-work").value });
    if (path) $("f-work").value = path;
  } catch (error) {
    showServiceError(error);
  }
}

async function saveService() {
  const edit = $("service-modal").dataset.edit === "1";
  const service = collectService();
  if (!service.name) {
    showServiceError("Service name is required.");
    return;
  }
  if (!service.executable) {
    showServiceError("Application is required.");
    return;
  }

  const button = $("btn-service-save");
  button.disabled = true;
  try {
    services = await invoke("save_service", { service, edit });
    closeModal("service-modal");
    selected = service.name;
    render();
    toast(edit ? "Service config updated." : "Service added.", "success");
    loadLog(true);
  } catch (error) {
    showServiceError(error);
  } finally {
    button.disabled = false;
  }
}

function showServiceError(error) {
  const element = $("service-error");
  element.textContent = String(error);
  element.classList.remove("hidden");
}

async function deleteSelected() {
  const service = selectedService();
  if (!service) return;
  if (!confirm(`Delete service '${service.name}' from Windows and the config?`)) return;
  try {
    services = await invoke("delete_service", { name: service.name });
    selected = null;
    render();
    toast(`Service '${service.name}' deleted.`, "success");
  } catch (error) {
    toast(error, "error", 8000);
  }
}

async function installSelected() {
  const service = selectedService();
  if (!service) return;
  const startAfter = confirm(
    `Install service '${service.name}' now?\n\nClick OK to install and start it, or Cancel to install without starting.`
  );
  try {
    services = await invoke("install_service", {
      name: service.name,
      startAfter,
    });
    render();
    toast(`Service '${service.name}' installed.`, "success");
  } catch (error) {
    toast(error, "error", 8000);
  }
}

async function singleAction(command) {
  const service = selectedService();
  if (!service) return;
  const action = command.replace("_service", "");
  try {
    setStatus(`Running ${action} for ${service.name}...`);
    services = await invoke(command, { name: service.name });
    render();
    setStatus("Ready");
    toast(`${action} ${service.name} completed.`, "success");
    loadLog(true);
  } catch (error) {
    setStatus("Failed");
    toast(error, "error", 8000);
  }
}

async function runBatch(action) {
  if (batchRunning) return;
  batchRunning = true;
  updateButtons();
  $("batch-title").textContent = `Running ${action} for all services...`;
  $("batch-text").textContent = "Preparing...";
  $("batch-log").textContent = "";
  $("batch-progress").value = 0;
  $("batch-progress").max = 1;
  showModal("batch-modal");

  try {
    const summary = await invoke("run_batch_action", { action });
    $("batch-text").textContent =
      `Completed: ${summary.succeeded} succeeded, ${summary.failed} failed, ${summary.cancelled} cancelled.`;
    toast("Batch operation completed.", summary.failed ? "error" : "success");
  } catch (error) {
    $("batch-text").textContent = String(error);
    toast(error, "error", 8000);
  } finally {
    batchRunning = false;
    await refresh();
    setTimeout(() => closeModal("batch-modal"), 1200);
  }
}

function onBatchProgress(event) {
  const total = Math.max(1, event.total || 1);
  $("batch-progress").max = total;
  $("batch-progress").value = event.done || 0;
  const suffix = event.message ? ` - ${event.message}` : "";
  $("batch-text").textContent = `${event.done}/${total} ${event.name}: ${event.state}${suffix}`;
  const line = document.createElement("div");
  const icon =
    event.state === "succeeded" ? "OK" : event.state === "cancelled" ? "CANCEL" : "FAIL";
  line.textContent = `[${icon}] ${event.name}${suffix}`;
  $("batch-log").appendChild(line);
  $("batch-log").scrollTop = $("batch-log").scrollHeight;
}

async function openImport() {
  try {
    const candidates = await invoke("import_candidates");
    if (!candidates || candidates.length === 0) {
      toast("No new services to import.");
      return;
    }
    const list = $("import-list");
    list.innerHTML = candidates
      .map(
        (service) => `<label>
          <input type="checkbox" data-name="${escapeHtml(service.name)}" checked />
          <span><strong>${escapeHtml(service.displayName || service.name)}</strong>
          <span class="muted"> [${escapeHtml(service.name)}] ${escapeHtml(typeLabel(service.serviceType))}</span></span>
        </label>`
      )
      .join("");
    $("import-count").textContent = `${candidates.length} candidate(s)`;
    showModal("import-modal");
  } catch (error) {
    toast(error, "error", 8000);
  }
}

async function confirmImport() {
  const names = [...$("import-list").querySelectorAll("input[type=checkbox]")]
    .filter((input) => input.checked)
    .map((input) => input.dataset.name);
  if (names.length === 0) {
    toast("Select at least one service.", "error");
    return;
  }
  try {
    services = await invoke("import_services", { names });
    closeModal("import-modal");
    render();
    toast(`${names.length} service(s) imported.`, "success");
  } catch (error) {
    toast(error, "error", 8000);
  }
}

async function backupConfig() {
  try {
    const path = await invoke("export_config");
    toast(`Backup created:\n${path}`, "success", 8000);
  } catch (error) {
    toast(error, "error");
  }
}

async function restoreConfigFile(file) {
  if (!file) return;
  try {
    const content = await file.text();
    services = await invoke("restore_config", { content });
    selected = null;
    render();
    toast("Config restored successfully.", "success");
    loadLog(true);
  } catch (error) {
    toast(error, "error", 8000);
  } finally {
    $("restore-file").value = "";
  }
}

async function showAbout() {
  try {
    const paths = await invoke("get_paths");
    $("about-paths").textContent = Object.entries(paths)
      .map(([key, value]) => `${key}: ${value}`)
      .join("\n");
    showModal("about-modal");
  } catch (error) {
    toast(error, "error");
  }
}

async function openSelectedLog() {
  const service = selectedService();
  if (!service) return;
  try {
    await invoke("open_log", { name: service.name });
  } catch (error) {
    toast(error, "error");
  }
}

function showContextMenu(event, name) {
  event.preventDefault();
  selected = name;
  render();
  const menu = $("service-context-menu");
  menu.classList.remove("hidden");
  updateContextMenuState();

  const bounds = menu.getBoundingClientRect();
  const left = Math.min(event.clientX, window.innerWidth - bounds.width - 8);
  const top = Math.min(event.clientY, window.innerHeight - bounds.height - 8);
  menu.style.left = `${Math.max(8, left)}px`;
  menu.style.top = `${Math.max(8, top)}px`;
}

function hideContextMenu() {
  $("service-context-menu").classList.add("hidden");
}

function updateContextMenuState() {
  const menu = $("service-context-menu");
  if (!menu) return;
  const service = selectedService();
  const installed = Boolean(service && service.installed);
  const running = Boolean(service && service.state === "running");
  const busy = batchRunning;
  const setDisabled = (action, disabled) => {
    const button = menu.querySelector(`button[data-action="${action}"]`);
    if (button) button.disabled = disabled;
  };
  setDisabled("edit", !service || busy);
  setDisabled("install", !service || installed || busy);
  setDisabled("start", !installed || running || busy);
  setDisabled("stop", !installed || !running || busy);
  setDisabled("restart", !installed || busy);
  setDisabled("log", !service);
  setDisabled("delete", !service || busy);
}

async function handleContextAction(action) {
  hideContextMenu();
  switch (action) {
    case "edit":
      openServiceModal(true);
      break;
    case "install":
      await installSelected();
      break;
    case "start":
      await singleAction("start_service");
      break;
    case "stop":
      await singleAction("stop_service");
      break;
    case "restart":
      await singleAction("restart_service");
      break;
    case "log":
      await openSelectedLog();
      break;
    case "delete":
      await deleteSelected();
      break;
  }
}

function bindEvents() {
  $("btn-add").addEventListener("click", () => openServiceModal(false));
  $("btn-refresh").addEventListener("click", refresh);
  $("btn-import").addEventListener("click", openImport);
  $("btn-start-all").addEventListener("click", () => runBatch("start"));
  $("btn-stop-all").addEventListener("click", () => runBatch("stop"));
  $("btn-restart-all").addEventListener("click", () => runBatch("restart"));
  $("btn-backup").addEventListener("click", backupConfig);
  $("btn-restore").addEventListener("click", () => $("restore-file").click());
  $("restore-file").addEventListener("change", (event) =>
    restoreConfigFile(event.target.files[0])
  );
  $("btn-open-folder").addEventListener("click", () =>
    invoke("open_app_folder").catch((error) => toast(error, "error"))
  );
  $("btn-app-log").addEventListener("click", () =>
    invoke("open_app_log").catch((error) => toast(error, "error"))
  );
  $("btn-about").addEventListener("click", showAbout);
  $("btn-elevate").addEventListener("click", () =>
    invoke("elevate").catch((error) => toast(error, "error"))
  );
  $("btn-service-save").addEventListener("click", saveService);
  $("btn-service-cancel").addEventListener("click", () => closeModal("service-modal"));
  $("btn-browse-exe").addEventListener("click", browseExecutable);
  $("btn-browse-work").addEventListener("click", browseWorkingDirectory);
  $("btn-import-confirm").addEventListener("click", confirmImport);
  $("btn-batch-cancel").addEventListener("click", () => {
    invoke("cancel_batch");
    $("batch-text").textContent = "Cancelling... running operations will finish.";
  });
  $("btn-log-reload").addEventListener("click", () => loadLog(true));
  $("btn-log-open").addEventListener("click", openSelectedLog);
  $("f-type").addEventListener("change", updateNativeFields);

  document.querySelectorAll("[data-close]").forEach((button) => {
    button.addEventListener("click", () => closeModal(button.dataset.close));
  });

  $("service-body").addEventListener("click", (event) => {
    const row = event.target.closest("tr[data-name]");
    if (row) selectService(row.dataset.name);
  });
  $("service-body").addEventListener("dblclick", (event) => {
    const row = event.target.closest("tr[data-name]");
    if (row) {
      selected = row.dataset.name;
      openServiceModal(true);
    }
  });
  $("service-body").addEventListener("contextmenu", (event) => {
    const row = event.target.closest("tr[data-name]");
    if (row) showContextMenu(event, row.dataset.name);
    else hideContextMenu();
  });

  $("service-context-menu").addEventListener("click", (event) => {
    const button = event.target.closest("button[data-action]");
    if (!button || button.disabled) return;
    handleContextAction(button.dataset.action);
  });

  document.addEventListener("click", (event) => {
    if (!event.target.closest("#service-context-menu")) hideContextMenu();
  });
  window.addEventListener("resize", hideContextMenu);
  window.addEventListener("scroll", hideContextMenu, true);

  document.addEventListener("keydown", (event) => {
    if (event.key === "Escape") {
      hideContextMenu();
      document.querySelectorAll(".modal:not(.hidden)").forEach((modal) => {
        if (modal.id !== "batch-modal") modal.classList.add("hidden");
      });
    }
  });
}

async function init() {
  bindEvents();
  try {
    admin = await invoke("is_admin");
    if (!admin) $("admin-banner").classList.remove("hidden");
  } catch (error) {
    toast(error, "error");
  }

  try {
    const paths = await invoke("get_paths");
    $("config-home").textContent = paths.home;
  } catch (error) {
    toast(error, "error");
  }

  await refresh();
  const warning = await invoke("get_warning");
  if (warning) toast(warning, "error", 10000);

  await listen("batch-started", (event) => {
    const total = event.payload?.total || 1;
    $("batch-progress").max = total;
    $("batch-progress").value = 0;
    batchRunning = true;
    updateButtons();
  });
  await listen("batch-progress", (event) => onBatchProgress(event.payload));
  await listen("batch-finished", () => {
    batchRunning = false;
    updateButtons();
  });

  refreshTimer = setInterval(() => {
    if (!batchRunning && document.visibilityState === "visible") refresh();
  }, 3000);
  logTimer = setInterval(() => {
    if ($("log-auto").checked && selected && document.visibilityState === "visible") {
      loadLog(false);
    }
  }, 2000);
}

window.addEventListener("DOMContentLoaded", init);
