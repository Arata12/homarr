(function () {
  function toast(msg, kind) {
    const stack = document.getElementById("toast-stack");
    if (!stack) return;
    const el = document.createElement("div");
    el.className = "toast " + (kind || "ok");
    el.textContent = msg;
    stack.appendChild(el);
    setTimeout(() => el.remove(), 4200);
  }
  window.homarrToast = toast;

  // Theme toggle persisted via localStorage
  const savedTheme = localStorage.getItem("homarr-theme");
  if (savedTheme === "light" || savedTheme === "dark") {
    document.documentElement.setAttribute("data-theme", savedTheme);
  }
  window.toggleTheme = function () {
    const cur = document.documentElement.getAttribute("data-theme") || "dark";
    const next = cur === "dark" ? "light" : "dark";
    document.documentElement.setAttribute("data-theme", next);
    localStorage.setItem("homarr-theme", next);
  };

  window.setLocaleCookie = function (locale) {
    document.cookie =
      "homarr-locale=" +
      encodeURIComponent(locale) +
      "; path=/; max-age=31536000; SameSite=Lax";
    location.reload();
  };

  window.fillDemoLogin = function () {
    const u = document.querySelector('#login-form input[name="name"]');
    const p = document.querySelector('#login-form input[name="password"]');
    if (u) u.value = "demo";
    if (p) p.value = "demo";
  };

  window.toggleManageDrawer = function () {
    document.body.classList.toggle("manage-drawer-open");
  };
  window.closeManageDrawer = function () {
    document.body.classList.remove("manage-drawer-open");
  };

  // Password visibility toggles
  document.addEventListener("click", function (e) {
    const btn = e.target.closest("[data-toggle-password]");
    if (!btn) return;
    const wrap = btn.closest(".password-wrap");
    const input = wrap ? wrap.querySelector("input") : null;
    if (!input) return;
    const showing = input.type === "text";
    input.type = showing ? "password" : "text";
    btn.classList.toggle("is-visible", !showing);
  });

  // Dropdown popovers (Board Switcher, User Menu)
  function closeAllDropdowns() {
    document.querySelectorAll(".dropdown.open").forEach((d) => {
      d.classList.remove("open");
      const btn = d.querySelector("[aria-expanded]");
      if (btn) btn.setAttribute("aria-expanded", "false");
    });
  }

  window.toggleDropdown = function (btn) {
    const parent = btn.closest(".dropdown");
    if (!parent) return;
    const willOpen = !parent.classList.contains("open");
    closeAllDropdowns();
    if (willOpen) {
      parent.classList.add("open");
      btn.setAttribute("aria-expanded", "true");
    }
  };

  document.addEventListener("click", function (e) {
    if (!e.target.closest(".dropdown")) {
      closeAllDropdowns();
    }
  });

  // Spotlight modal (Ctrl+K / Cmd+K) with categories, actions, and keyboard navigation
  const spotModal = document.getElementById("spotlight-modal");
  const spotInput = document.getElementById("spotlight-input");
  const spotResults = document.getElementById("spotlight-results");
  let spotIndex = 0;
  let spotCategory = "all";

  document.querySelectorAll(".spotlight-tab[data-spotlight-cat]").forEach((tab) => {
    tab.addEventListener("click", function () {
      spotCategory = tab.getAttribute("data-spotlight-cat") || "all";
      document
        .querySelectorAll(".spotlight-tab[data-spotlight-cat]")
        .forEach((t) => t.classList.toggle("is-active", t === tab));
      renderSpotlight(spotInput ? spotInput.value : "");
      if (spotInput) spotInput.focus();
    });
  });

  function openSpotlight() {
    if (!spotModal) return;
    closeAllDropdowns();
    spotModal.classList.add("open");
    spotIndex = 0;
    if (spotInput) {
      spotInput.value = "";
      spotInput.focus();
      renderSpotlight("");
    }
  }

  function closeSpotlight() {
    if (spotModal) spotModal.classList.remove("open");
  }
  window.openSpotlight = openSpotlight;
  window.closeSpotlight = closeSpotlight;

  function updateSpotlightSelection() {
    if (!spotResults) return;
    const links = spotResults.querySelectorAll(".spotlight-item");
    links.forEach((el, idx) => {
      el.classList.toggle("is-selected", idx === spotIndex);
    });
  }

  function renderSpotlight(query) {
    if (!spotResults) return;
    const q = (query || "").trim().toLowerCase();
    const raw = window.__HOMARR_SPOTLIGHT__ || [];
    const filtered = raw
      .filter((item) => {
        const catOk = spotCategory === "all" || item.category === spotCategory;
        const qOk =
          !q ||
          (item.label + " " + (item.sub || "") + " " + (item.category || ""))
            .toLowerCase()
            .includes(q);
        return catOk && qOk;
      })
      .slice(0, 12);
    spotResults.innerHTML = "";
    spotIndex = 0;

    if (filtered.length === 0 && q) {
      const a = document.createElement("a");
      a.className = "spotlight-item is-selected";
      a.href = "https://duckduckgo.com/?q=" + encodeURIComponent(query);
      a.target = "_blank";
      a.rel = "noopener noreferrer";
      const lbl = document.createElement("span");
      lbl.textContent = "Search web for: " + query;
      const badge = document.createElement("span");
      badge.className = "badge";
      badge.textContent = "DuckDuckGo";
      a.appendChild(lbl);
      a.appendChild(badge);
      spotResults.appendChild(a);
      return;
    }

    filtered.forEach((item, idx) => {
      const a = document.createElement("a");
      a.className = "spotlight-item" + (idx === 0 ? " is-selected" : "");
      if (item.action === "toggle-theme" || item.action === "edit-board") {
        a.href = "#";
        a.addEventListener("click", function (ev) {
          ev.preventDefault();
          if (item.action === "toggle-theme" && window.toggleTheme) window.toggleTheme();
          if (item.action === "edit-board" && window.toggleEditMode) window.toggleEditMode();
          closeSpotlight();
        });
      } else {
        const rawHref = typeof item.href === "string" ? item.href.trim() : "";
        const lowerHref = rawHref.toLowerCase();
        a.href =
          rawHref &&
          !rawHref.startsWith("//") &&
          (lowerHref.startsWith("http://") ||
            lowerHref.startsWith("https://") ||
            rawHref.startsWith("/"))
            ? rawHref
            : "#";
      }
      const lbl = document.createElement("span");
      lbl.textContent = item.label;
      const badge = document.createElement("span");
      badge.className = "badge";
      badge.textContent = item.sub || item.category || "Link";
      a.appendChild(lbl);
      a.appendChild(badge);
      spotResults.appendChild(a);
    });
  }

  document.addEventListener("keydown", function (e) {
    if ((e.ctrlKey || e.metaKey) && e.key.toLowerCase() === "k") {
      e.preventDefault();
      if (spotModal && spotModal.classList.contains("open")) closeSpotlight();
      else openSpotlight();
    } else if (e.key === "Escape") {
      closeSpotlight();
      closeAllDropdowns();
      window.closeManageDrawer();
    } else if (spotModal && spotModal.classList.contains("open") && spotResults) {
      const links = spotResults.querySelectorAll(".spotlight-item");
      if (links.length === 0) return;
      if (e.key === "ArrowDown") {
        e.preventDefault();
        spotIndex = (spotIndex + 1) % links.length;
        updateSpotlightSelection();
      } else if (e.key === "ArrowUp") {
        e.preventDefault();
        spotIndex = (spotIndex - 1 + links.length) % links.length;
        updateSpotlightSelection();
      } else if (e.key === "Enter" && links[spotIndex]) {
        e.preventDefault();
        links[spotIndex].click();
      }
    }
  });
  if (spotInput) {
    spotInput.addEventListener("input", () => renderSpotlight(spotInput.value));
  }

  // Live clock & countdown & interactive timer updates
  const timerIntervals = new WeakMap();

  function formatMmSs(totalSecs) {
    const m = Math.floor(totalSecs / 60);
    const s = totalSecs % 60;
    return String(m).padStart(2, "0") + ":" + String(s).padStart(2, "0");
  }

  window.toggleTimer = function (btn) {
    const widget = btn.closest(".timer-widget");
    if (!widget) return;
    const existing = timerIntervals.get(widget);
    if (existing) {
      clearInterval(existing);
      timerIntervals.delete(widget);
      btn.textContent = "Start";
      return;
    }
    btn.textContent = "Pause";
    const id = setInterval(() => {
      let rem = Number(widget.getAttribute("data-remaining") || 0);
      if (rem <= 0) {
        clearInterval(id);
        timerIntervals.delete(widget);
        btn.textContent = "Start";
        toast("Timer finished", "ok");
        return;
      }
      rem -= 1;
      widget.setAttribute("data-remaining", String(rem));
      const disp = widget.querySelector(".js-timer-display");
      if (disp) disp.textContent = formatMmSs(rem);
    }, 1000);
    timerIntervals.set(widget, id);
  };

  window.resetTimer = function (btn) {
    const widget = btn.closest(".timer-widget");
    if (!widget) return;
    const existing = timerIntervals.get(widget);
    if (existing) {
      clearInterval(existing);
      timerIntervals.delete(widget);
    }
    const base = Number(widget.getAttribute("data-duration") || 1500);
    widget.setAttribute("data-remaining", String(base));
    const disp = widget.querySelector(".js-timer-display");
    if (disp) disp.textContent = formatMmSs(base);
    const startBtn = widget.querySelector("button");
    if (startBtn) startBtn.textContent = "Start";
  };

  function updateClocks() {
    const now = new Date();
    document.querySelectorAll(".clock-widget").forEach((el) => {
      const is24 = el.getAttribute("data-24h") !== "false";
      const timeEl = el.querySelector(".clock-time");
      if (timeEl) {
        timeEl.textContent = now.toLocaleTimeString([], {
          hour: "2-digit",
          minute: "2-digit",
          hour12: !is24,
        });
      }
      const dateEl = el.querySelector(".clock-date");
      if (dateEl) {
        dateEl.textContent = now.toLocaleDateString([], {
          weekday: "short",
          month: "short",
          day: "numeric",
        });
      }
    });
    document.querySelectorAll(".countdown-widget").forEach((el) => {
      const rawDate = el.getAttribute("data-target-date");
      const target = rawDate ? Date.parse(rawDate) : NaN;
      const disp = el.querySelector(".js-countdown");
      if (!isNaN(target) && disp) {
        const diff = Math.max(0, Math.floor((target - Date.now()) / 1000));
        const d = Math.floor(diff / 86400);
        const h = Math.floor((diff % 86400) / 3600);
        const m = Math.floor((diff % 3600) / 60);
        disp.textContent = d + "d " + h + "h " + m + "m";
      }
    });
  }
  updateClocks();
  setInterval(updateClocks, 1000);

  // Collapsible category sections persisted in localStorage
  const COLLAPSED_KEY = "homarr-collapsed-sections";
  function readCollapsedMap() {
    try {
      return JSON.parse(localStorage.getItem(COLLAPSED_KEY) || "{}");
    } catch (_) {
      return {};
    }
  }
  const collapsedMap = readCollapsedMap();
  document.querySelectorAll(".category-section[data-section-id]").forEach((sec) => {
    const id = sec.getAttribute("data-section-id");
    if (id && collapsedMap[id]) {
      sec.setAttribute("data-collapsed", "true");
      const btn = sec.querySelector(".category-header");
      if (btn) btn.setAttribute("aria-expanded", "false");
    }
  });

  window.toggleCategory = function (btn) {
    const sec = btn.closest(".category-section");
    if (!sec) return;
    const isCollapsed = sec.getAttribute("data-collapsed") === "true";
    const next = !isCollapsed;
    sec.setAttribute("data-collapsed", next ? "true" : "false");
    btn.setAttribute("aria-expanded", next ? "false" : "true");
    const id = sec.getAttribute("data-section-id");
    if (id) {
      const map = readCollapsedMap();
      if (next) map[id] = true;
      else delete map[id];
      localStorage.setItem(COLLAPSED_KEY, JSON.stringify(map));
    }
  };

  // Generic declarative API form handler ([data-api-form])
  document.querySelectorAll("form[data-api-form]").forEach((form) => {
    form.addEventListener("submit", async function (e) {
      e.preventDefault();
      const url = form.getAttribute("data-api-form");
      const method = (form.getAttribute("data-method") || "POST").toUpperCase();
      const redirectTo = form.getAttribute("data-redirect");
      const clearEmpty = form.hasAttribute("data-clear-empty");
      const revealSel = form.getAttribute("data-reveal-key");
      const payload = {};
      const secrets = [];

      form.querySelectorAll("[data-secret-kind]").forEach((el) => {
        const kind = el.getAttribute("data-secret-kind");
        if (kind && el.value.trim() !== "") {
          secrets.push({ kind, value: el.value.trim() });
        }
      });

      form.querySelectorAll("input[name], select[name], textarea[name]").forEach((el) => {
        const k = el.name;
        if (!k || el.hasAttribute("data-secret-kind")) return;
        if (el.type === "checkbox") {
          payload[k] = Boolean(el.checked);
        } else if (el.type === "range" || el.type === "number") {
          if (el.value !== "") payload[k] = Number(el.value);
        } else if (k === "secretsJson") {
          if (el.value.trim()) {
            try {
              const parsed = JSON.parse(el.value);
              if (Array.isArray(parsed)) {
                parsed.forEach((s) => secrets.push(s));
              } else if (parsed && typeof parsed === "object") {
                Object.entries(parsed).forEach(([sk, sv]) => {
                  if (sv !== null && sv !== undefined && String(sv) !== "") {
                    secrets.push({ kind: sk, value: String(sv) });
                  }
                });
              }
            } catch (_) {}
          }
        } else if (el.value !== "") {
          payload[k] = el.value;
        } else if (clearEmpty && el.tagName !== "SELECT") {
          payload[k] = null;
        }
      });

      if (secrets.length > 0) {
        payload.secrets = secrets;
      }

      const resp = await fetch(url, {
        method,
        headers: { "Content-Type": "application/json" },
        body: JSON.stringify(payload),
      });
      const data = await resp.json().catch(() => ({}));
      if (resp.ok) {
        if (revealSel && data.apiKey) {
          const box = document.querySelector(revealSel);
          const valEl = document.getElementById("api-key-value");
          const copyBtn = document.getElementById("copy-api-key-btn");
          if (box && valEl) {
            valEl.textContent = data.apiKey;
            box.hidden = false;
            if (copyBtn) {
              copyBtn.onclick = () => {
                navigator.clipboard && navigator.clipboard.writeText(data.apiKey);
                toast("API Key copied", "ok");
              };
            }
          }
          const tbody = document.querySelector("#api-keys-table tbody");
          if (tbody && data.record) {
            const emptyRow = tbody.querySelector("td[colspan]");
            if (emptyRow) emptyRow.closest("tr").remove();
            const tr = document.createElement("tr");
            const td1 = document.createElement("td");
            const code = document.createElement("code");
            code.textContent = data.record.id;
            td1.appendChild(code);
            const td2 = document.createElement("td");
            const badge = document.createElement("span");
            badge.className = "badge";
            badge.textContent = data.record.readOnly ? "Read-Only" : "Read/Write";
            td2.appendChild(badge);
            const td3 = document.createElement("td");
            const del = document.createElement("button");
            del.type = "button";
            del.className = "btn btn-danger btn-sm";
            del.setAttribute("data-api-action", "/api/api-keys/" + encodeURIComponent(data.record.id));
            del.setAttribute("data-method", "DELETE");
            del.textContent = "Revoke";
            td3.appendChild(del);
            tr.append(td1, td2, td3);
            tbody.appendChild(tr);
          }
          toast("API Key created", "ok");
          return;
        }
        toast("Saved successfully", "ok");
        if (redirectTo) location.href = redirectTo;
        else location.reload();
      } else {
        toast(
          (data.error && data.error.message) || "Request failed (" + resp.status + ")",
          "error"
        );
      }
    });
  });

  // Nested server settings form ([data-settings-form])
  document.querySelectorAll("form[data-settings-form]").forEach((form) => {
    form.addEventListener("submit", async function (e) {
      e.preventDefault();
      const url = form.getAttribute("data-settings-form");
      const patch = {};
      form.querySelectorAll("input[name], select[name], textarea[name]").forEach((el) => {
        const parts = el.name.split(".");
        if (parts.length !== 2) return;
        const [sec, key] = parts;
        patch[sec] = patch[sec] || {};
        if (el.type === "checkbox") {
          patch[sec][key] = Boolean(el.checked);
        } else if (el.tagName === "SELECT" && el.value === "") {
          patch[sec][key] = null;
        } else {
          patch[sec][key] = el.value.trim() || (el.name.endsWith("Url") ? null : "");
        }
      });
      const resp = await fetch(url, {
        method: "PATCH",
        headers: { "Content-Type": "application/json" },
        body: JSON.stringify(patch),
      });
      const data = await resp.json().catch(() => ({}));
      if (resp.ok) {
        toast("Server settings saved", "ok");
        setTimeout(() => location.reload(), 400);
      } else {
        toast((data.error && data.error.message) || "Failed to save settings", "error");
      }
    });
  });

  // Dynamic integration credential fields
  const intKindSelect = document.getElementById("integration-kind-select");
  const intSecretContainer = document.getElementById("integration-secret-fields");
  const intUrlInput = document.getElementById("integration-url-input");
  if (intKindSelect && intSecretContainer && window.__HOMARR_INTEGRATION_DEFS__) {
    const renderIntFields = () => {
      const def = window.__HOMARR_INTEGRATION_DEFS__[intKindSelect.value];
      intSecretContainer.innerHTML = "";
      if (!def) return;
      if (intUrlInput && def.defaultUrl) {
        intUrlInput.placeholder = def.defaultUrl;
      }
      if (!def.secrets || def.secrets.length === 0) {
        const p = document.createElement("p");
        p.className = "muted";
        p.style.cssText = "grid-column:1 / -1;margin:0;font-size:.82rem";
        p.textContent = "No additional credentials required for " + def.name + ".";
        intSecretContainer.appendChild(p);
        return;
      }
      def.secrets.forEach((s) => {
        const row = document.createElement("div");
        row.className = "form-row";
        const lbl = document.createElement("label");
        lbl.textContent = s.label + (s.optional ? " (optional)" : "");
        const input = document.createElement(s.multiline ? "textarea" : "input");
        if (!s.multiline) input.type = s.isPublic ? "text" : "password";
        else input.rows = 3;
        input.setAttribute("data-secret-kind", s.kind);
        input.placeholder = s.label;
        row.append(lbl, input);
        intSecretContainer.appendChild(row);
      });
    };
    intKindSelect.addEventListener("change", renderIntFields);
    renderIntFields();
  }

  // Table search filter ([data-table-filter])
  document.querySelectorAll("[data-table-filter]").forEach((inp) => {
    const sel = inp.getAttribute("data-table-filter");
    inp.addEventListener("input", () => {
      const q = inp.value.trim().toLowerCase();
      const tbl = document.querySelector(sel);
      if (!tbl) return;
      tbl.querySelectorAll("tbody tr").forEach((tr) => {
        if (tr.querySelector("td[colspan]")) return;
        tr.style.display = !q || tr.textContent.toLowerCase().includes(q) ? "" : "none";
      });
    });
  });

  // List search filter ([data-list-filter])
  document.querySelectorAll("[data-list-filter]").forEach((inp) => {
    const sel = inp.getAttribute("data-list-filter");
    inp.addEventListener("input", () => {
      const q = inp.value.trim().toLowerCase();
      const list = document.querySelector(sel);
      if (!list) return;
      Array.from(list.children).forEach((el) => {
        el.style.display = !q || el.textContent.toLowerCase().includes(q) ? "" : "none";
      });
    });
  });

  // Bundled icon search ([data-icon-search])
  document.querySelectorAll("[data-icon-search]").forEach((inp) => {
    const resultsEl = document.querySelector(inp.getAttribute("data-icon-search"));
    const targetEl = document.querySelector(inp.getAttribute("data-icon-target"));
    let timer = null;
    inp.addEventListener("input", () => {
      clearTimeout(timer);
      timer = setTimeout(async () => {
        if (!resultsEl) return;
        const q = inp.value.trim();
        if (!q) {
          resultsEl.innerHTML = "";
          return;
        }
        const r = await fetch("/api/icons?q=" + encodeURIComponent(q)).catch(() => null);
        if (!r || !r.ok) return;
        const data = await r.json().catch(() => ({}));
        resultsEl.innerHTML = "";
        (data.items || []).slice(0, 24).forEach((icon) => {
          const btn = document.createElement("button");
          btn.type = "button";
          btn.title = icon.name || icon.url;
          const img = document.createElement("img");
          img.src = icon.url;
          img.alt = icon.name || "";
          btn.appendChild(img);
          btn.onclick = () => {
            if (targetEl) targetEl.value = icon.url;
            toast("Selected icon: " + (icon.name || icon.url), "ok");
          };
          resultsEl.appendChild(btn);
        });
      }, 220);
    });
  });

  // Edit Application modal ([data-edit-app])
  document.addEventListener("click", function (e) {
    const btn = e.target.closest("[data-edit-app]");
    if (!btn) return;
    let app;
    try {
      app = JSON.parse(btn.getAttribute("data-edit-app") || "{}");
    } catch (_) {
      return;
    }
    const backdrop = document.createElement("div");
    backdrop.className = "modal-backdrop open";
    const card = document.createElement("div");
    card.className = "modal-card";
    const head = document.createElement("div");
    head.className = "modal-head";
    const title = document.createElement("h3");
    title.textContent = "Edit Application";
    const closeBtn = document.createElement("button");
    closeBtn.type = "button";
    closeBtn.className = "icon-btn icon-btn-sm";
    closeBtn.textContent = "✕";
    closeBtn.onclick = () => backdrop.remove();
    head.append(title, closeBtn);

    const form = document.createElement("form");
    form.className = "form-grid";
    form.style.maxWidth = "none";
    const mkRow = (label, name, val, req) => {
      const r = document.createElement("div");
      r.className = "form-row";
      const l = document.createElement("label");
      l.textContent = label;
      const i = document.createElement("input");
      i.name = name;
      i.value = val || "";
      if (req) i.required = true;
      r.append(l, i);
      return { r, i };
    };
    const fName = mkRow("Name", "name", app.name, true);
    const fIcon = mkRow("Icon URL", "iconUrl", app.iconUrl, true);
    const fHref = mkRow("URL", "href", app.href, false);
    const fDesc = mkRow("Description", "description", app.description, false);
    const actions = document.createElement("div");
    actions.style.cssText = "display:flex;gap:8px;justify-content:flex-end";
    const cancelBtn = document.createElement("button");
    cancelBtn.type = "button";
    cancelBtn.className = "btn";
    cancelBtn.textContent = "Cancel";
    cancelBtn.onclick = () => backdrop.remove();
    const saveBtn = document.createElement("button");
    saveBtn.type = "submit";
    saveBtn.className = "btn btn-primary";
    saveBtn.textContent = "Save Changes";
    actions.append(cancelBtn, saveBtn);
    form.append(fName.r, fIcon.r, fHref.r, fDesc.r, actions);
    form.onsubmit = async (ev) => {
      ev.preventDefault();
      const resp = await fetch("/api/apps/" + encodeURIComponent(app.id), {
        method: "PATCH",
        headers: { "Content-Type": "application/json" },
        body: JSON.stringify({
          name: fName.i.value.trim(),
          iconUrl: fIcon.i.value.trim(),
          href: fHref.i.value.trim() || null,
          description: fDesc.i.value.trim() || null,
        }),
      });
      if (resp.ok) {
        toast("Application updated", "ok");
        location.reload();
      } else {
        toast("Update failed", "error");
      }
    };
    card.append(head, form);
    backdrop.appendChild(card);
    backdrop.onclick = (ev) => {
      if (ev.target === backdrop) backdrop.remove();
    };
    document.body.appendChild(backdrop);
  });

  // Logs terminal level & text filtering
  const logLevelSel = document.getElementById("log-level-filter");
  const logTextInp = document.getElementById("log-text-filter");
  const logTerminal = document.getElementById("log-terminal");
  if (logTerminal && (logLevelSel || logTextInp)) {
    const filterLogs = () => {
      const lvl = logLevelSel ? logLevelSel.value : "all";
      const q = logTextInp ? logTextInp.value.trim().toLowerCase() : "";
      logTerminal.querySelectorAll(".log-line").forEach((line) => {
        const lineLvl = line.getAttribute("data-level") || "info";
        const lvlOk = lvl === "all" || lineLvl === lvl;
        const qOk = !q || line.textContent.toLowerCase().includes(q);
        line.style.display = lvlOk && qOk ? "" : "none";
      });
    };
    if (logLevelSel) logLevelSel.addEventListener("change", filterLogs);
    if (logTextInp) logTextInp.addEventListener("input", filterLogs);
  }

  // Cron job enable/pause toggles ([data-cron-toggle])
  document.addEventListener("click", async function (e) {
    const btn = e.target.closest("[data-cron-toggle]");
    if (!btn) return;
    const name = btn.getAttribute("data-cron-toggle");
    const enabled = btn.getAttribute("data-enabled") === "true";
    const resp = await fetch("/api/cron-jobs/" + encodeURIComponent(name), {
      method: "PATCH",
      headers: { "Content-Type": "application/json" },
      body: JSON.stringify({ isEnabled: enabled }),
    });
    if (resp.ok) {
      toast("Task schedule updated", "ok");
      location.reload();
    } else {
      toast("Failed to update task", "error");
    }
  });

  // Backup file picker & restore submit
  const backupFile = document.getElementById("backup-file-input");
  const backupJson = document.getElementById("backup-json-input");
  if (backupFile && backupJson) {
    backupFile.addEventListener("change", async () => {
      const f = backupFile.files && backupFile.files[0];
      if (f) backupJson.value = await f.text();
    });
  }
  const restoreBtn = document.querySelector("[data-restore-backup]");
  if (restoreBtn && backupJson) {
    restoreBtn.addEventListener("click", async () => {
      let parsed;
      try {
        parsed = JSON.parse(backupJson.value);
      } catch (_) {
        toast("Invalid backup JSON", "error");
        return;
      }
      const resp = await fetch("/api/backup/import", {
        method: "POST",
        headers: { "Content-Type": "application/json" },
        body: JSON.stringify(parsed),
      });
      if (resp.ok) {
        toast("Backup restored", "ok");
        setTimeout(() => location.reload(), 600);
      } else {
        toast("Backup restore failed", "error");
      }
    });
  }

  // Immediate live ping check for unknown app tiles on board load
  const pendingAppIds = [];
  document.querySelectorAll(".app-tile[data-app-id]").forEach((tile) => {
    const appId = (tile.getAttribute("data-app-id") || "").trim();
    if (appId && tile.querySelector(".ping-dot.unknown") && !pendingAppIds.includes(appId) && pendingAppIds.length < 16) {
      pendingAppIds.push(appId);
    }
  });
  pendingAppIds.forEach((appId) => {
    fetch("/api/apps/ping", {
      method: "POST",
      headers: { "Content-Type": "application/json" },
      body: JSON.stringify({ appId }),
    })
      .then((r) => (r.ok ? r.json() : null))
      .then((res) => {
        if (!res) return;
        const ok = res.statusCode && res.statusCode < 400;
        document
          .querySelectorAll('.app-tile[data-app-id="' + CSS.escape(appId) + '"] .ping-dot')
          .forEach((dot) => {
            dot.classList.remove("unknown", "up", "down");
            dot.classList.add(ok ? "up" : "down");
          });
      })
      .catch(() => {});
  });

  // Generic action buttons ([data-api-action])
  document.addEventListener("click", async function (e) {
    const btn = e.target.closest("[data-api-action]");
    if (!btn) return;
    const url = btn.getAttribute("data-api-action");
    const method = (btn.getAttribute("data-method") || "POST").toUpperCase();
    const resp = await fetch(url, { method });
    const data = await resp.json().catch(() => ({}));
    if (resp.ok) {
      toast("Action completed", "ok");
      if (btn.getAttribute("data-reload") !== "false") location.reload();
    } else {
      toast((data.error && data.error.message) || "Action failed", "error");
    }
  });

  // Live WebSocket events
  try {
    const proto = location.protocol === "https:" ? "wss:" : "ws:";
    const ws = new WebSocket(proto + "//" + location.host + "/websockets");
    ws.onmessage = function (ev) {
      try {
        const msg = JSON.parse(ev.data);
        if (msg.channel === "homarr:ping:events" && msg.payload && msg.payload.url) {
          document
            .querySelectorAll('[data-ping-url="' + CSS.escape(String(msg.payload.url)) + '"] .ping-dot')
            .forEach((dot) => {
              dot.classList.remove("up", "down", "unknown");
              dot.classList.add(
                msg.payload.statusCode && msg.payload.statusCode < 400 ? "up" : "down"
              );
            });
        }
      } catch (_) {}
    };
  } catch (_) {}
})();
