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
  return service.restartOnCrash ? `Ya (${service.restartDelaySec}s)` : "Tidak";
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
  $("counts").textContent = `${services.length} terdaftar | ${installed} terpasang | ${running} berjalan`;
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

  $("btn-edit").disabled = !service || busy;
  $("btn-delete").disabled = !service || busy;
  $("btn-install").disabled = !service || installed || busy;
  $("btn-start").disabled = !installed || running || busy;
  $("btn-stop").disabled = !installed || !running || busy;
  $("btn-restart").disabled = !installed || busy;
  $("btn-start-all").disabled = !hasInstalled || busy;
  $("btn-stop-all").disabled = !hasInstalled || busy;
  $("btn-restart-all").disabled = !hasInstalled || busy;
  $("btn-log-open").disabled = !service;
  $("btn-log-reload").disabled = !service;
}

function updateLogHeader() {
  const service = selectedService();
  if (!service) {
    $("log-title").textContent = "Log service: (belum ada yang dipilih)";
    $("log-path").textContent = "";
    return;
  }
  $("log-title").textContent = `Log service: ${service.name}`;
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
    toast("Pilih service yang akan diedit.", "error");
    return;
  }
  const service = edit ? selectedService() : emptyService();
  $("service-modal-title").textContent = edit ? "Edit Service" : "Tambah Service";
  $("service-hint").textContent = edit
    ? "Nama service tidak dapat diubah saat edit."
    : "Service dapat dipasang setelah config disimpan.";
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

async function saveService() {
  const edit = $("service-modal").dataset.edit === "1";
  const service = collectService();
  if (!service.name) {
    showServiceError("Nama service wajib diisi.");
    return;
  }
  if (!service.executable) {
    showServiceError("Application wajib diisi.");
    return;
  }

  const button = $("btn-service-save");
  button.disabled = true;
  try {
    services = await invoke("save_service", { service, edit });
    closeModal("service-modal");
    selected = service.name;
    render();
    toast(edit ? "Config service diperbarui." : "Service ditambahkan.", "success");
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
  if (!confirm(`Hapus service '${service.name}' dari Windows dan config?`)) return;
  try {
    services = await invoke("delete_service", { name: service.name });
    selected = null;
    render();
    toast(`Service '${service.name}' dihapus.`, "success");
  } catch (error) {
    toast(error, "error", 8000);
  }
}

async function installSelected() {
  const service = selectedService();
  if (!service) return;
  const startAfter = confirm(
    `Install service '${service.name}' sekarang?\n\nKlik OK untuk install dan langsung jalankan, Cancel untuk install tanpa start.`
  );
  try {
    services = await invoke("install_service", {
      name: service.name,
      startAfter,
    });
    render();
    toast(`Service '${service.name}' berhasil dipasang.`, "success");
  } catch (error) {
    toast(error, "error", 8000);
  }
}

async function singleAction(command) {
  const service = selectedService();
  if (!service) return;
  try {
    setStatus(`Menjalankan ${command} untuk ${service.name}...`);
    services = await invoke(command, { name: service.name });
    render();
    setStatus("Siap");
    toast(`${command} ${service.name} selesai.`, "success");
    loadLog(true);
  } catch (error) {
    setStatus("Gagal");
    toast(error, "error", 8000);
  }
}

async function runBatch(action) {
  if (batchRunning) return;
  batchRunning = true;
  updateButtons();
  $("batch-title").textContent = `Menjalankan ${action} semua service...`;
  $("batch-text").textContent = "Menyiapkan...";
  $("batch-log").textContent = "";
  $("batch-progress").value = 0;
  $("batch-progress").max = 1;
  showModal("batch-modal");

  try {
    const summary = await invoke("run_batch_action", { action });
    $("batch-text").textContent =
      `Selesai: ${summary.succeeded} sukses, ${summary.failed} gagal, ${summary.cancelled} dibatalkan.`;
    toast("Operasi batch selesai.", summary.failed ? "error" : "success");
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
      toast("Tidak ada service baru untuk diimpor.");
      return;
    }
    const list = $("import-list");
    list.innerHTML = candidates
      .map(
        (service, index) => `<label>
          <input type="checkbox" data-name="${escapeHtml(service.name)}" checked />
          <span><strong>${escapeHtml(service.displayName || service.name)}</strong>
          <span class="muted"> [${escapeHtml(service.name)}] ${escapeHtml(typeLabel(service.serviceType))}</span></span>
        </label>`
      )
      .join("");
    $("import-count").textContent = `${candidates.length} kandidat`;
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
    toast("Centang minimal satu service.", "error");
    return;
  }
  try {
    services = await invoke("import_services", { names });
    closeModal("import-modal");
    render();
    toast(`${names.length} service diimpor.`, "success");
  } catch (error) {
    toast(error, "error", 8000);
  }
}

async function backupConfig() {
  try {
    const path = await invoke("export_config");
    toast(`Backup dibuat:\n${path}`, "success", 8000);
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
    toast("Config berhasil dipulihkan.", "success");
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

function bindEvents() {
  $("btn-add").addEventListener("click", () => openServiceModal(false));
  $("btn-edit").addEventListener("click", () => openServiceModal(true));
  $("btn-delete").addEventListener("click", deleteSelected);
  $("btn-install").addEventListener("click", installSelected);
  $("btn-refresh").addEventListener("click", refresh);
  $("btn-import").addEventListener("click", openImport);
  $("btn-start").addEventListener("click", () => singleAction("start_service"));
  $("btn-stop").addEventListener("click", () => singleAction("stop_service"));
  $("btn-restart").addEventListener("click", () => singleAction("restart_service"));
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
  $("btn-import-confirm").addEventListener("click", confirmImport);
  $("btn-batch-cancel").addEventListener("click", () => {
    invoke("cancel_batch");
    $("batch-text").textContent = "Membatalkan... service yang sedang berjalan akan diselesaikan.";
  });
  $("btn-log-reload").addEventListener("click", () => loadLog(true));
  $("btn-log-open").addEventListener("click", () => {
    const service = selectedService();
    if (!service) return;
    invoke("open_log", { name: service.name }).catch((error) => toast(error, "error"));
  });
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

  document.addEventListener("keydown", (event) => {
    if (event.key === "Escape") {
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
