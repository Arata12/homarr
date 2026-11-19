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

  // Spotlight modal (Ctrl+K / Cmd+K) with keyboard navigation
  const spotModal = document.getElementById("spotlight-modal");
  const spotInput = document.getElementById("spotlight-input");
  const spotResults = document.getElementById("spotlight-results");
  let spotIndex = 0;

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
      .filter(
        (item) =>
          !q ||
          (item.label + " " + (item.sub || "")).toLowerCase().includes(q)
      )
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
      a.href = item.href || "#";
      const lbl = document.createElement("span");
      lbl.textContent = item.label;
      const badge = document.createElement("span");
      badge.className = "badge";
      badge.textContent = item.sub || "Link";
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
      const addModal = document.getElementById("add-item-modal");
      if (addModal) addModal.classList.remove("open");
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

  // Board edit mode & Widget Picker helpers
  window.toggleEditMode = function () {
    document.body.classList.toggle("edit-mode");
  };

  window.openAddItemModal = function () {
    const m = document.getElementById("add-item-modal");
    if (m) {
      m.classList.add("open");
      const search = document.getElementById("widget-picker-search");
      if (search) {
        search.value = "";
        window.filterWidgetPicker("");
        search.focus();
      }
    }
  };

  window.filterWidgetPicker = function (query) {
    const q = (query || "").trim().toLowerCase();
    document.querySelectorAll(".widget-picker-card").forEach((card) => {
      const blob = card.getAttribute("data-search") || "";
      card.style.display = !q || blob.includes(q) ? "" : "none";
    });
  };

  window.selectWidgetKind = function (btn, kind) {
    document
      .querySelectorAll(".widget-picker-card.is-selected")
      .forEach((el) => el.classList.remove("is-selected"));
    btn.classList.add("is-selected");
    const form = btn.closest("form");
    if (!form) return;
    const hidden = form.querySelector('input[name="kind"]');
    if (hidden) hidden.value = kind;
    const appRow = document.getElementById("picker-app-row");
    if (appRow) appRow.style.display = kind === "app" ? "" : "none";
  };

  window.submitAddBoardItem = async function (e, boardId, defaultSectionId, layoutId) {
    e.preventDefault();
    const form = e.target;
    const kind = form.querySelector("[name=kind]").value;
    const appSel = form.querySelector("[name=appId]");
    const appId = appSel ? appSel.value : "";
    const secSel = form.querySelector("[name=sectionId]");
    const sectionId = (secSel && secSel.value) || defaultSectionId;
    const wInput = form.querySelector("[name=width]");
    const hInput = form.querySelector("[name=height]");
    const width = Math.max(1, Number((wInput && wInput.value) || 2));
    const height = Math.max(1, Number((hInput && hInput.value) || 1));

    const options = kind === "app" && appId ? { appId } : {};
    const resp = await fetch("/api/boards/" + encodeURIComponent(boardId) + "/items", {
      method: "POST",
      headers: { "Content-Type": "application/json" },
      body: JSON.stringify({
        id: "",
        kind,
        options,
        advancedOptions: {},
        integrationIds: [],
        layouts: [
          {
            itemId: "",
            sectionId,
            layoutId,
            xOffset: 0,
            yOffset: 0,
            width,
            height,
          },
        ],
      }),
    });
    if (resp.ok) {
      location.reload();
    } else {
      const err = await resp.json().catch(() => ({}));
      toast((err.error && err.error.message) || "Failed to add item", "error");
    }
  };

  window.deleteBoardItem = async function (boardId, itemId) {
    const resp = await fetch(
      "/api/boards/" + encodeURIComponent(boardId) + "/items/" + encodeURIComponent(itemId),
      { method: "DELETE" }
    );
    if (resp.ok) location.reload();
    else toast("Failed to remove item", "error");
  };

  // Generic declarative API form handler ([data-api-form])
  document.querySelectorAll("form[data-api-form]").forEach((form) => {
    form.addEventListener("submit", async function (e) {
      e.preventDefault();
      const url = form.getAttribute("data-api-form");
      const method = (form.getAttribute("data-method") || "POST").toUpperCase();
      const redirectTo = form.getAttribute("data-redirect");
      const payload = {};
      form.querySelectorAll("input[name], select[name], textarea[name]").forEach((el) => {
        const k = el.name;
        if (!k) return;
        if (el.type === "checkbox") {
          payload[k] = Boolean(el.checked);
        } else if (el.type === "range" || el.type === "number") {
          if (el.value !== "") payload[k] = Number(el.value);
        } else if (k === "secretsJson") {
          if (el.value) {
            try {
              payload.secrets = JSON.parse(el.value);
            } catch (_) {}
          }
        } else if (el.value !== "") {
          payload[k] = el.value;
        }
      });

      const resp = await fetch(url, {
        method,
        headers: { "Content-Type": "application/json" },
        body: JSON.stringify(payload),
      });
      const data = await resp.json().catch(() => ({}));
      if (resp.ok) {
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
            .querySelectorAll('[data-ping-url="' + msg.payload.url + '"] .ping-dot')
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
