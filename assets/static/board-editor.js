/*
 * Homarr board editor (loaded only for users who may modify the board).
 *
 * Data model: window.__HOMARR_EDITOR__ (see crates/homarr-web/src/pages/board_editor.rs).
 * Persistence: PUT /api/boards/{id}/layout (sections + items), PUT /api/boards/{id}/layouts.
 * Positions are stored per responsive layout; item.p[layoutId] = [x, y, w, h, real].
 */
(function () {
  "use strict";
  const E = window.__HOMARR_EDITOR__;
  if (!E) return;

  const ROW_FALLBACK = 108;
  const MAX_H = 32;
  const T = (k) => (E.i18n && E.i18n[k]) || k;
  const toast = (msg, kind) => {
    if (window.homarrToast) window.homarrToast(msg, kind);
  };

  // ---------------------------------------------------------------- helpers
  function h(tag, props, ...kids) {
    const el = document.createElement(tag);
    for (const [k, v] of Object.entries(props || {})) {
      if (v === undefined || v === null || v === false) continue;
      if (k === "class") el.className = v;
      else if (k === "text") el.textContent = v;
      else if (k === "html") el.innerHTML = v; // only used with trusted, server-provided icons
      else if (k.startsWith("on") && typeof v === "function") el.addEventListener(k.slice(2), v);
      else if (k === "value") el.value = v;
      else if (k === "checked") el.checked = Boolean(v);
      else el.setAttribute(k, v === true ? "" : String(v));
    }
    for (const kid of kids.flat()) {
      if (kid === null || kid === undefined || kid === false) continue;
      el.appendChild(typeof kid === "string" ? document.createTextNode(kid) : kid);
    }
    return el;
  }

  function newId() {
    const bytes = new Uint8Array(20);
    crypto.getRandomValues(bytes);
    let out = "c";
    for (const b of bytes) out += (b % 36).toString(36);
    return out;
  }

  const clone = (v) => JSON.parse(JSON.stringify(v));
  const clamp = (v, lo, hi) => Math.max(lo, Math.min(hi, v));
  const store = {
    get(k) {
      try {
        return sessionStorage.getItem(k);
      } catch (_) {
        return null;
      }
    },
    set(k, v) {
      try {
        if (v === null) sessionStorage.removeItem(k);
        else sessionStorage.setItem(k, v);
      } catch (_) {}
    },
  };
  const EDIT_KEY = "homarr-edit-mode";
  const LAYOUT_KEY = "homarr-edit-layout:" + E.boardId;

  // ------------------------------------------------------------------ model
  const layouts = E.layouts; // sorted by breakpoint ascending
  const items = new Map();
  E.items.forEach((it) => items.set(it.id, it));
  const sections = E.sections;
  const widgetsByKind = new Map(E.widgets.map((w) => [w.kind, w]));
  let curLayoutId = null;

  function pickInitialLayout() {
    const saved = store.get(LAYOUT_KEY);
    if (saved && layouts.some((l) => l.id === saved)) return saved;
    let chosen = layouts[0];
    for (const l of layouts) if (window.innerWidth >= l.breakpoint) chosen = l;
    return chosen ? chosen.id : null;
  }

  const curLayout = () => layouts.find((l) => l.id === curLayoutId) || layouts[0];
  const curCols = () => (curLayout() ? curLayout().columnCount : 12);

  function sortedSections() {
    return sections
      .slice()
      .sort((a, b) => (a.yOffset ?? 0) - (b.yOffset ?? 0) || (a.xOffset ?? 0) - (b.xOffset ?? 0));
  }

  function placement(it) {
    let p = it.p[curLayoutId];
    if (!p) {
      const w = clamp(2, 1, curCols());
      p = [0, 0, w, 1, 0];
      it.p[curLayoutId] = p;
    }
    return p;
  }

  // it.o[layoutId] holds a stored placement in a section other than the rendered one. The editor
  // can't show it, so it is saved back untouched unless the user moves this item in that layout.
  function claim(it) {
    if (it.o) delete it.o[curLayoutId];
  }

  /** Moves an item to another section in the edited layout only; other layouts keep theirs. */
  function changeSection(it, sectionId) {
    it.o = it.o || {};
    for (const l of layouts) {
      const p = it.p[l.id];
      if (l.id === curLayoutId || it.o[l.id] || !p || !p[4]) continue;
      it.o[l.id] = {
        itemId: it.id,
        sectionId: it.sectionId,
        layoutId: l.id,
        xOffset: p[0],
        yOffset: p[1],
        width: p[2],
        height: p[3],
      };
      p[4] = 0;
    }
    claim(it);
    it.sectionId = sectionId;
  }

  function itemsIn(sectionId, excludeId) {
    const out = [];
    for (const it of items.values()) {
      if (it.sectionId === sectionId && it.id !== excludeId) out.push(it);
    }
    return out;
  }

  const overlap = (a, b) =>
    a[0] < b[0] + b[2] && b[0] < a[0] + a[2] && a[1] < b[1] + b[3] && b[1] < a[1] + a[3];

  /** Vertical compaction around an optional pinned rectangle. Returns Map id -> [x,y,w,h]. */
  function settle(sectionId, pinned, excludeId) {
    const cols = curCols();
    const list = itemsIn(sectionId, pinned ? pinned.id : excludeId).filter(
      (it) => !(pinned && it.id === pinned.id)
    );
    list.sort((a, b) => placement(a)[1] - placement(b)[1] || placement(a)[0] - placement(b)[0]);
    const placed = [];
    const out = new Map();
    if (pinned) {
      const r = [pinned.x, pinned.y, pinned.w, pinned.h];
      placed.push(r);
      out.set(pinned.id, r);
    }
    for (const it of list) {
      const p = placement(it);
      const w = clamp(p[2], 1, cols);
      const x = clamp(p[0], 0, cols - w);
      const hh = clamp(p[3], 1, MAX_H);
      let y = 0;
      while (placed.some((r) => overlap([x, y, w, hh], r))) y++;
      const r = [x, y, w, hh];
      placed.push(r);
      out.set(it.id, r);
    }
    return out;
  }

  function firstFree(sectionId, w, hh) {
    const cols = curCols();
    w = clamp(w, 1, cols);
    const occupied = itemsIn(sectionId).map((it) => placement(it).slice(0, 4));
    for (let y = 0; ; y++) {
      for (let x = 0; x <= cols - w; x++) {
        if (!occupied.some((r) => overlap([x, y, w, hh], r))) return [x, y, w, hh];
      }
    }
  }

  // -------------------------------------------------------------- DOM access
  function itemEl(id) {
    return document.querySelector('.grid-item[data-item-id="' + CSS.escape(id) + '"]');
  }
  function sectionEl(id) {
    return document.querySelector('.board-section[data-section-id="' + CSS.escape(id) + '"]');
  }
  function gridOf(sectionId) {
    const s = sectionEl(sectionId);
    return s ? s.querySelector(".board-grid") : null;
  }

  function applyView(id, rect) {
    const el = itemEl(id);
    if (!el) return;
    el.style.setProperty("--ex", rect[0]);
    el.style.setProperty("--ey", rect[1]);
    el.style.setProperty("--ew", rect[2]);
    el.style.setProperty("--eh", rect[3]);
  }

  function applyLayout() {
    document.body.dataset.editLayout = curLayoutId || "";
    document.body.style.setProperty("--ecols", String(curCols()));
    for (const it of items.values()) applyView(it.id, placement(it));
    updateCounts();
  }

  function updateCounts() {
    for (const s of sections) {
      const el = sectionEl(s.id);
      const c = el && el.querySelector(".category-count");
      if (c) c.textContent = String(itemsIn(s.id).length);
    }
  }

  // ------------------------------------------------------------- persistence
  let saveTimer = null;
  let saving = null;
  let pending = false;
  let pendingPromise = null;
  let dirty = false;
  let mutationRev = 0;
  const statusEl = () => document.getElementById("editor-save-status");

  function setStatus(text, isError) {
    const el = statusEl();
    if (!el) return;
    el.textContent = text;
    el.classList.toggle("is-error", Boolean(isError));
  }

  function buildPayload() {
    return {
      sections: sections.map((s) => ({
        id: s.id,
        kind: s.kind,
        xOffset: s.xOffset ?? 0,
        yOffset: s.yOffset ?? 0,
        name: s.name ?? null,
        options: s.options || {},
        layouts: s.layouts || [],
      })),
      items: [...items.values()].map((it) => ({
        id: it.id,
        kind: it.kind,
        options: it.options || {},
        advancedOptions: it.advancedOptions || {},
        integrationIds: it.integrationIds || [],
        layouts: layouts.flatMap((l) => {
          if (it.o && it.o[l.id]) return [it.o[l.id]];
          const p = it.p[l.id];
          if (!p || !p[4]) return [];
          return [
            {
              itemId: it.id,
              sectionId: it.sectionId,
              layoutId: l.id,
              xOffset: p[0],
              yOffset: p[1],
              width: p[2],
              height: p[3],
            },
          ];
        }),
      })),
    };
  }

  async function doSave() {
    const rev = mutationRev;
    setStatus(T("saving"));
    let resp;
    try {
      resp = await fetch("/api/boards/" + encodeURIComponent(E.boardId) + "/layout", {
        method: "PUT",
        headers: { "Content-Type": "application/json" },
        body: JSON.stringify(buildPayload()),
      });
    } catch (_) {
      setStatus(T("saveFailed"), true);
      throw new Error(T("saveFailed"));
    }
    if (!resp.ok) {
      const err = await resp.json().catch(() => ({}));
      const msg = (err.error && err.error.message) || T("saveFailed");
      setStatus(T("saveFailed"), true);
      toast(msg, "error");
      throw new Error(msg);
    }
    if (rev === mutationRev) {
      dirty = false;
    }
    setStatus(T("saved"));
  }

  function saveNow() {
    clearTimeout(saveTimer);
    saveTimer = null;
    if (saving) {
      pending = true;
      if (!pendingPromise) {
        pendingPromise = saving
          .catch(() => {})
          .then(() => {
            pendingPromise = null;
            if (pending) {
              pending = false;
              return saveNow();
            }
          });
      }
      return pendingPromise;
    }
    saving = doSave().finally(() => {
      saving = null;
      if (pending && !pendingPromise) {
        pending = false;
        saveNow().catch(() => {});
      }
    });
    return saving;
  }

  function scheduleSave() {
    mutationRev += 1;
    dirty = true;
    setStatus(T("saving"));
    clearTimeout(saveTimer);
    saveTimer = setTimeout(() => saveNow().catch(() => {}), 350);
  }

  async function saveAndReload() {
    mutationRev += 1;
    dirty = true;
    try {
      await saveNow();
      location.reload();
    } catch (_) {}
  }

  // -------------------------------------------------------------- edit mode
  function enterEdit() {
    document.body.classList.add("edit-mode");
    store.set(EDIT_KEY, E.boardId);
    if (!curLayoutId) curLayoutId = pickInitialLayout();
    buildLayoutSelect();
    applyLayout();
  }

  async function exitEdit() {
    if (dirty || saveTimer || pending || saving) {
      try {
        await saveNow();
      } catch (_) {
        return;
      }
    }
    store.set(EDIT_KEY, null);
    location.reload();
  }

  window.toggleEditMode = function () {
    if (document.body.classList.contains("edit-mode")) exitEdit();
    else enterEdit();
  };

  function buildLayoutSelect() {
    const sel = document.getElementById("editor-layout-select");
    if (!sel) return;
    sel.textContent = "";
    for (const l of layouts) {
      sel.appendChild(
        h("option", {
          value: l.id,
          text: l.name + " (" + l.columnCount + " cols, ≥" + l.breakpoint + "px)",
        })
      );
    }
    sel.value = curLayoutId;
    sel.onchange = async () => {
      if (dirty || saveTimer || pending || saving) {
        try {
          await saveNow();
        } catch (_) {
          sel.value = curLayoutId;
          return;
        }
      }
      store.set(LAYOUT_KEY, sel.value);
      location.reload();
    };
  }

  // ------------------------------------------------------------- modal infra
  const modalRoot = () => document.getElementById("editor-modal-root") || document.body;
  const modalStack = [];

  function openModal(title, build, opts) {
    const backdrop = h("div", { class: "modal-backdrop open editor-modal" });
    const card = h("div", { class: "modal-card" + (opts && opts.wide ? " modal-card-wide" : "") });
    const close = () => {
      backdrop.remove();
      const i = modalStack.indexOf(backdrop);
      if (i >= 0) modalStack.splice(i, 1);
    };
    card.appendChild(
      h(
        "div",
        { class: "modal-head" },
        h("h3", { text: title }),
        h("button", {
          type: "button",
          class: "icon-btn icon-btn-sm",
          "aria-label": T("close"),
          html: E.icons.close,
          onclick: close,
        })
      )
    );
    backdrop.appendChild(card);
    backdrop.addEventListener("mousedown", (e) => {
      if (e.target === backdrop) close();
    });
    modalRoot().appendChild(backdrop);
    modalStack.push(backdrop);
    build(card, close);
    return close;
  }

  document.addEventListener("keydown", (e) => {
    if (e.key === "Escape" && modalStack.length) {
      const top = modalStack[modalStack.length - 1];
      top.remove();
      modalStack.pop();
      e.stopPropagation();
    }
  });

  // ------------------------------------------------------------ field widgets
  function field(label, control, hint) {
    return h(
      "div",
      { class: "editor-field" },
      label ? h("label", { text: label }) : null,
      control,
      hint ? h("span", { class: "editor-hint", text: hint }) : null
    );
  }

  /** Builds a form control for a widget option. Returns { el, get }. */
  function optionControl(f, value, ctx) {
    const v = value === undefined ? f.defaultValue : value;
    switch (f.type) {
      case "switch": {
        const input = h("input", { type: "checkbox", checked: Boolean(v) });
        return {
          el: h(
            "div",
            { class: "editor-field editor-field-switch" },
            input,
            h("label", { text: f.label }),
            f.description ? h("span", { class: "editor-hint", text: f.description }) : null
          ),
          get: () => input.checked,
        };
      }
      case "number": {
        const input = h("input", {
          type: "number",
          value: v ?? "",
          min: f.min,
          max: f.max,
          step: f.step ?? "any",
        });
        return {
          el: field(f.label, input, f.description),
          get: () => (input.value === "" ? f.defaultValue : Number(input.value)),
        };
      }
      case "slider": {
        const out = h("span", { text: String(v) });
        const input = h("input", {
          type: "range",
          min: f.min ?? 0,
          max: f.max ?? 100,
          step: f.step ?? 1,
          value: v,
          oninput: () => (out.textContent = input.value),
        });
        return {
          el: field(f.label, h("div", { class: "editor-inline" }, input, out), f.description),
          get: () => Number(input.value),
        };
      }
      case "textarea": {
        const input = h("textarea", { rows: "5", value: v ?? "" });
        return { el: field(f.label, input, f.description), get: () => input.value };
      }
      case "select": {
        const input = h(
          "select",
          {},
          f.choices.map((c) => h("option", { value: c.value, text: c.label }))
        );
        input.value = v;
        return { el: field(f.label, input, f.description), get: () => input.value };
      }
      case "multiSelect": {
        const selected = new Set(Array.isArray(v) ? v : []);
        const boxes = f.choices.map((c) => {
          const cb = h("input", { type: "checkbox", checked: selected.has(c.value) });
          return { c, cb, el: h("label", {}, cb, c.label) };
        });
        return {
          el: field(f.label, h("div", { class: "editor-checks" }, boxes.map((b) => b.el)), f.description),
          get: () => boxes.filter((b) => b.cb.checked).map((b) => b.c.value),
        };
      }
      case "textList": {
        const input = h("textarea", {
          rows: "4",
          value: (Array.isArray(v) ? v : []).join("\n"),
        });
        return {
          el: field(f.label, input, f.description || T("onePerLine")),
          get: () =>
            input.value
              .split("\n")
              .map((s) => s.trim())
              .filter(Boolean),
        };
      }
      case "app":
        return appControl(f, v, ctx);
      case "location": {
        const loc = v && typeof v === "object" ? v : { name: "", latitude: 0, longitude: 0 };
        const name = h("input", { type: "text", value: loc.name ?? "", placeholder: T("locationName") });
        const lat = h("input", { type: "number", step: "any", min: "-90", max: "90", value: loc.latitude });
        const lon = h("input", { type: "number", step: "any", min: "-180", max: "180", value: loc.longitude });
        return {
          el: field(
            f.label,
            h(
              "div",
              { class: "editor-inline" },
              name,
              lat,
              lon
            ),
            f.description || T("latitude") + " / " + T("longitude")
          ),
          get: () => ({
            name: name.value,
            latitude: Number(lat.value),
            longitude: Number(lon.value),
          }),
        };
      }
      case "json": {
        const input = h("textarea", {
          rows: "6",
          class: "editor-json",
          value: JSON.stringify(v ?? null, null, 2),
        });
        return {
          el: field(f.label, input, f.description || T("jsonValue")),
          get: () => {
            try {
              return JSON.parse(input.value);
            } catch (_) {
              throw new Error(f.label + ": " + T("invalidJson"));
            }
          },
        };
      }
      default: {
        const input = h("input", { type: "text", value: v ?? "" });
        return { el: field(f.label, input, f.description), get: () => input.value };
      }
    }
  }

  function appOptionList(select, selected) {
    select.textContent = "";
    select.appendChild(h("option", { value: "", text: "—" }));
    for (const a of E.apps) select.appendChild(h("option", { value: a.id, text: a.name }));
    select.value = selected || "";
  }

  function appControl(f, value, ctx) {
    const select = h("select", {});
    appOptionList(select, value);
    const edit = h("button", {
      type: "button",
      class: "btn btn-sm",
      text: T("editApp"),
      onclick: () => {
        const app = E.apps.find((a) => a.id === select.value);
        if (!app) return toast(T("noAppSelected"), "error");
        openAppEditor(app, (saved) => {
          appOptionList(select, saved.id);
          ctx && ctx.onAppChanged && ctx.onAppChanged();
        });
      },
    });
    const create = h("button", {
      type: "button",
      class: "btn btn-sm",
      text: T("newApp"),
      onclick: () =>
        openAppEditor(null, (saved) => {
          appOptionList(select, saved.id);
          ctx && ctx.onAppChanged && ctx.onAppChanged();
        }),
    });
    return {
      el: field(f.label, h("div", { class: "editor-inline" }, select, create, edit), f.description),
      get: () => select.value,
    };
  }

  // ------------------------------------------------------------- app editor
  function openAppEditor(app, onSaved) {
    openModal(app ? T("editApp") : T("newApp"), (card, close) => {
      const name = h("input", { type: "text", required: true, maxlength: "64", value: app ? app.name : "" });
      const icon = h("input", { type: "text", value: app ? app.iconUrl : "", placeholder: "https://…/icon.svg" });
      const preview = h("img", { class: "editor-icon-preview", alt: "", src: app ? app.iconUrl : "" });
      icon.addEventListener("input", () => (preview.src = icon.value));
      const href = h("input", { type: "url", value: (app && app.href) || "", placeholder: "https://" });
      const ping = h("input", { type: "url", value: (app && app.pingUrl) || "", placeholder: "https://" });
      const desc = h("input", { type: "text", maxlength: "512", value: (app && app.description) || "" });
      const results = h("div", { class: "editor-icon-results" });
      const search = h("input", { type: "search", placeholder: T("iconSearch") });
      let timer = null;
      const runSearch = async () => {
        const q = search.value.trim();
        results.textContent = "";
        if (!q) return;
        try {
          const r = await fetch("/api/icons?limit=48&q=" + encodeURIComponent(q));
          const data = await r.json();
          for (const ic of data.items || []) {
            results.appendChild(
              h(
                "button",
                {
                  type: "button",
                  title: ic.name,
                  onclick: () => {
                    icon.value = ic.url;
                    preview.src = ic.url;
                  },
                },
                h("img", { src: ic.url, alt: ic.name, loading: "lazy" })
              )
            );
          }
        } catch (_) {}
      };
      search.addEventListener("input", () => {
        clearTimeout(timer);
        timer = setTimeout(runSearch, 250);
      });
      const favicon = h("button", {
        type: "button",
        class: "btn btn-sm",
        text: "Favicon",
        onclick: async () => {
          if (!href.value) return;
          try {
            const r = await fetch("/api/icons/favicon?url=" + encodeURIComponent(href.value));
            const data = await r.json();
            if (data.iconUrl) {
              icon.value = data.iconUrl;
              preview.src = data.iconUrl;
            }
          } catch (_) {}
        },
      });
      const form = h(
        "form",
        {
          class: "editor-form",
          onsubmit: async (e) => {
            e.preventDefault();
            const body = {
              name: name.value.trim(),
              description: desc.value.trim() || null,
              iconUrl: icon.value.trim(),
              href: href.value.trim() || null,
              pingUrl: ping.value.trim() || null,
            };
            if (!body.name || !body.iconUrl) return toast(T("name") + " / " + T("iconUrl"), "error");
            const r = await fetch(app ? "/api/apps/" + encodeURIComponent(app.id) : "/api/apps", {
              method: app ? "PATCH" : "POST",
              headers: { "Content-Type": "application/json" },
              body: JSON.stringify(body),
            });
            const data = await r.json().catch(() => ({}));
            if (!r.ok) return toast((data.error && data.error.message) || "Request failed", "error");
            const idx = E.apps.findIndex((a) => a.id === data.id);
            if (idx >= 0) E.apps[idx] = data;
            else E.apps.push(data);
            close();
            onSaved && onSaved(data);
          },
        },
        field(T("name"), name),
        field(T("iconUrl"), h("div", { class: "editor-inline" }, preview, icon, favicon)),
        field(T("iconSearch"), search),
        results,
        field(T("appUrl"), href),
        field(T("pingUrl"), ping),
        field(T("description"), desc),
        h(
          "div",
          { class: "editor-actions" },
          h("button", { type: "button", class: "btn", text: T("cancel"), onclick: close }),
          h("button", { type: "submit", class: "btn btn-primary", text: T("save") })
        )
      );
      card.appendChild(form);
    });
  }

  // ------------------------------------------------------------ item editor
  function openItemEditor(opts) {
    const existing = opts.item || null;
    const kind = existing ? existing.kind : opts.kind;
    const meta = widgetsByKind.get(kind);
    if (!meta) return toast("Unknown widget: " + kind, "error");

    openModal(meta.name, (card, close) => {
      const options = existing ? existing.options || {} : clone(meta.defaultOptions || {});
      if (opts.presetOptions) Object.assign(options, opts.presetOptions);
      const adv = existing ? existing.advancedOptions || {} : {};
      const controls = [];
      const form = h("form", { class: "editor-form" });
      form.appendChild(h("p", { class: "muted", text: meta.description, style: "margin:0" }));

      // Integrations
      let integrationBoxes = [];
      const visibleIntegrationIds = new Set(E.integrations.map((i) => i.id));
      const hiddenIntegrationIds = existing
        ? (existing.integrationIds || []).filter((id) => !visibleIntegrationIds.has(id))
        : [];
      if (meta.supportedIntegrations.length) {
        const compatible = E.integrations.filter((i) => meta.supportedIntegrations.includes(i.kind));
        const selected = new Set(existing ? existing.integrationIds : []);
        integrationBoxes = compatible.map((i) => {
          const cb = h("input", { type: "checkbox", checked: selected.has(i.id) });
          return { i, cb, el: h("label", {}, cb, i.name + " (" + i.kind + ")") };
        });
        form.appendChild(
          field(
            T("integrations"),
            compatible.length
              ? h("div", { class: "editor-checks" }, integrationBoxes.map((b) => b.el))
              : h(
                  "div",
                  { class: "muted" },
                  T("noIntegrations") + " ",
                  h("a", { href: "/manage/integrations", text: T("createIntegration") })
                )
          )
        );
      }

      // Options generated from the widget definition
      for (const f of meta.fields) {
        const c = optionControl(f, options[f.key], {});
        controls.push({ f, c });
        form.appendChild(c.el);
      }

      // Placement
      const cols = curCols();
      const secSelect = h(
        "select",
        {},
        sortedSections().map((s) => h("option", { value: s.id, text: s.name || (s.kind === "category" ? "Category" : "Main") }))
      );
      secSelect.value = existing ? existing.sectionId : opts.sectionId || sortedSections()[0].id;
      const curP = existing ? placement(existing) : null;
      const wInput = h("input", {
        type: "number",
        min: "1",
        max: String(cols),
        value: curP ? curP[2] : Math.min(meta.defaultWidth, cols),
      });
      const hInput = h("input", {
        type: "number",
        min: "1",
        max: String(MAX_H),
        value: curP ? curP[3] : meta.defaultHeight,
      });
      form.appendChild(
        h(
          "div",
          { class: "editor-inline" },
          field(T("section"), secSelect),
          field(T("width"), wInput),
          field(T("height"), hInput)
        )
      );

      // Advanced options
      const title = h("input", { type: "text", maxlength: "64", value: adv.title || "" });
      const classes = h("input", {
        type: "text",
        placeholder: "my-class other-class",
        value: (adv.customCssClasses || []).join(" "),
      });
      let border = adv.borderColor || "";
      const color = h("input", { type: "color", value: border || "#fa5252" });
      color.addEventListener("input", () => (border = color.value));
      const clearBorder = h("button", {
        type: "button",
        class: "btn btn-sm",
        text: T("clear"),
        onclick: () => (border = ""),
      });
      form.appendChild(
        h(
          "details",
          { class: "editor-details" },
          h("summary", { text: T("advancedOptions") }),
          h(
            "div",
            { class: "editor-form" },
            field(T("title"), title),
            field(T("customCssClasses"), classes),
            field(T("borderColor"), h("div", { class: "editor-inline" }, color, clearBorder))
          )
        )
      );

      form.appendChild(
        h(
          "div",
          { class: "editor-actions" },
          h("button", { type: "button", class: "btn", text: T("cancel"), onclick: close }),
          h("button", { type: "submit", class: "btn btn-primary", text: existing ? T("save") : T("add") })
        )
      );

      form.addEventListener("submit", async (e) => {
        e.preventDefault();
        let newOptions;
        try {
          newOptions = Object.assign({}, options);
          for (const { f, c } of controls) newOptions[f.key] = c.get();
        } catch (err) {
          return toast(err.message, "error");
        }
        const advanced = {
          title: title.value.trim() || null,
          customCssClasses: classes.value.split(/[\s,]+/).filter(Boolean),
          borderColor: border,
        };
        const integrationIds = [
          ...new Set([
            ...hiddenIntegrationIds,
            ...integrationBoxes.filter((b) => b.cb.checked).map((b) => b.i.id),
          ]),
        ];
        const w = clamp(parseInt(wInput.value, 10) || 1, 1, curCols());
        const hh = clamp(parseInt(hInput.value, 10) || 1, 1, MAX_H);
        const sectionId = secSelect.value;

        let it = existing;
        if (!it) {
          it = {
            id: newId(),
            kind,
            options: {},
            advancedOptions: {},
            integrationIds: [],
            sectionId,
            p: {},
          };
          it.p[curLayoutId] = firstFree(sectionId, w, hh).concat(1);
          const baseLayout = layouts.find((l) => l.role === "base") || layouts[0];
          if (baseLayout && baseLayout.id !== curLayoutId) {
            const bw = clamp(w, 1, Math.max(1, baseLayout.columnCount || 1));
            const bh = clamp(hh, 1, MAX_H);
            it.p[baseLayout.id] = [0, 0, bw, bh, 1];
          }
          items.set(it.id, it);
        } else {
          const p = placement(it);
          if (sectionId !== it.sectionId) {
            changeSection(it, sectionId);
            it.p[curLayoutId] = firstFree(sectionId, w, hh).concat(1);
          } else if (w !== p[2] || hh !== p[3]) {
            claim(it);
            const res = settle(sectionId, { id: it.id, x: clamp(p[0], 0, curCols() - w), y: p[1], w, h: hh });
            for (const [id, r] of res) items.get(id).p[curLayoutId] = r.concat(1);
          }
        }
        it.options = newOptions;
        it.advancedOptions = advanced;
        it.integrationIds = integrationIds;
        close();
        await saveAndReload();
      });
      card.appendChild(form);
    }, { wide: true });
  }

  // ------------------------------------------------------------ widget picker
  function openPicker(sectionId) {
    openModal(T("addWidget"), (card, close) => {
      const search = h("input", { type: "search", placeholder: T("search"), autocomplete: "off" });
      const tabs = h("div", { class: "picker-cat-tabs" });
      const grid = h("div", { class: "widget-picker-grid" });
      const appsTitle = h("h4", { text: T("yourApps"), style: "margin:12px 0 6px" });
      const appGrid = h("div", { class: "widget-picker-grid" });
      const cardsEl = [];
      let activeCat = "all";
      const pick = (kind, preset) => {
        close();
        openItemEditor({ kind, sectionId, presetOptions: preset });
      };
      for (const w of E.widgets) {
        const el = h(
          "button",
          { type: "button", class: "widget-picker-card", onclick: () => pick(w.kind) },
          h("div", { class: "widget-picker-top" }, h("strong", { text: w.name }), h("span", { class: "widget-picker-kind", text: w.kind })),
          h("div", { class: "widget-picker-desc", text: w.description })
        );
        cardsEl.push({
          el,
          cat: w.category || "general",
          blob: (w.name + " " + w.kind + " " + w.description).toLowerCase(),
        });
        grid.appendChild(el);
      }
      const appCards = [];
      for (const a of E.apps) {
        const el = h(
          "button",
          { type: "button", class: "widget-picker-card", onclick: () => pick("app", { appId: a.id }) },
          h("div", { class: "widget-picker-top" }, h("strong", { text: a.name }), h("span", { class: "widget-picker-kind", text: "app" })),
          h("div", { class: "widget-picker-desc", text: a.description || a.href || "" })
        );
        appCards.push({ el, cat: "apps", blob: (a.name + " " + (a.description || "")).toLowerCase() });
        appGrid.appendChild(el);
      }
      function applyFilter() {
        const q = search.value.trim().toLowerCase();
        for (const c of cardsEl.concat(appCards)) {
          const catOk = activeCat === "all" || c.cat === activeCat;
          const qOk = !q || c.blob.includes(q);
          c.el.style.display = catOk && qOk ? "" : "none";
        }
      }
      const cats = ["all", ...new Set(E.widgets.map((w) => w.category || "general"))];
      if (E.apps.length) cats.push("apps");
      for (const cat of cats) {
        const label = cat === "all" ? "All" : cat.charAt(0).toUpperCase() + cat.slice(1);
        const btn = h("button", {
          type: "button",
          class: "spotlight-tab" + (cat === "all" ? " is-active" : ""),
          text: label,
          onclick: () => {
            activeCat = cat;
            tabs.querySelectorAll(".spotlight-tab").forEach((b) => b.classList.toggle("is-active", b === btn));
            applyFilter();
          },
        });
        tabs.appendChild(btn);
      }
      search.addEventListener("input", applyFilter);
      const newApp = h("button", {
        type: "button",
        class: "btn btn-sm",
        html: E.icons.plus + "<span></span>",
        onclick: () => {
          close();
          openAppEditor(null, (saved) => openItemEditor({ kind: "app", sectionId, presetOptions: { appId: saved.id } }));
        },
      });
      newApp.querySelector("span").textContent = T("newApp");
      card.appendChild(
        h(
          "div",
          { class: "editor-form" },
          h("div", { class: "editor-inline" }, search, newApp),
          tabs,
          grid,
          E.apps.length ? appsTitle : null,
          E.apps.length ? appGrid : null
        )
      );
      setTimeout(() => search.focus(), 0);
    }, { wide: true });
  }

  // ------------------------------------------------------------ section ops
  function renumberSections(order) {
    order.forEach((s, i) => {
      s.yOffset = i;
    });
  }

  function openSectionEditor(section) {
    openModal(section ? T("editSection") : T("addSection"), (card, close) => {
      const name = h("input", { type: "text", maxlength: "64", value: section ? section.name || "" : "" });
      const kind = h(
        "select",
        {},
        h("option", { value: "category", text: T("sectionKindCategory") }),
        h("option", { value: "empty", text: T("sectionKindEmpty") })
      );
      kind.value = section ? (section.kind === "category" ? "category" : "empty") : "category";
      const form = h(
        "form",
        {
          class: "editor-form",
          onsubmit: async (e) => {
            e.preventDefault();
            const nm = name.value.trim();
            if (section) {
              section.name = nm || null;
              section.kind = kind.value;
            } else {
              const order = sortedSections();
              const s = {
                id: newId(),
                kind: kind.value,
                name: nm || null,
                xOffset: 0,
                yOffset: order.length,
                options: {},
                layouts: [],
              };
              sections.push(s);
            }
            close();
            await saveAndReload();
          },
        },
        field(T("sectionName"), name),
        field("", kind),
        h(
          "div",
          { class: "editor-actions" },
          h("button", { type: "button", class: "btn", text: T("cancel"), onclick: close }),
          h("button", { type: "submit", class: "btn btn-primary", text: T("save") })
        )
      );
      card.appendChild(form);
      setTimeout(() => name.focus(), 0);
    });
  }

  async function moveSection(id, delta) {
    const order = sortedSections();
    const i = order.findIndex((s) => s.id === id);
    const j = i + delta;
    if (i < 0 || j < 0 || j >= order.length) return;
    [order[i], order[j]] = [order[j], order[i]];
    renumberSections(order);
    await saveAndReload();
  }

  async function removeSection(id) {
    if (sections.length <= 1) return toast(T("lastSection"), "error");
    if (!confirm(T("confirmRemoveSection"))) return;
    for (const it of [...items.values()]) {
      for (const k of Object.keys(it.o || {})) if (it.o[k].sectionId === id) delete it.o[k];
      if (it.sectionId !== id) continue;
      // Keep the item while another layout still places it in a different section.
      if (!Object.keys(it.o || {}).length) items.delete(it.id);
      else for (const l of layouts) if (it.p[l.id]) it.p[l.id][4] = 0;
    }
    sections.splice(
      sections.findIndex((s) => s.id === id),
      1
    );
    renumberSections(sortedSections());
    await saveAndReload();
  }

  // ---------------------------------------------------------- layouts dialog
  function openLayoutsEditor() {
    openModal(T("layouts"), (card, close) => {
      const rows = [];
      const list = h("div", { class: "editor-form" });
      const addRow = (l) => {
        const protectedRow = l && l.role !== "custom";
        const name = h("input", { type: "text", maxlength: "64", value: l ? l.name : "", "aria-label": T("layoutName") });
        const bp = h("input", {
          type: "number",
          min: "0",
          max: "32767",
          value: l ? l.breakpoint : 1024,
          disabled: l && l.role === "mobile",
          "aria-label": T("breakpoint"),
        });
        const cols = h("input", {
          type: "number",
          min: "1",
          max: "24",
          value: l ? l.columnCount : 12,
          "aria-label": T("columnCount"),
        });
        const row = { l, name, bp, cols, el: null, removed: false };
        row.el = h(
          "div",
          { class: "editor-layout-row" },
          field(T("layoutName"), name),
          field(T("breakpoint"), bp),
          field(T("columnCount"), cols),
          h("button", {
            type: "button",
            class: "icon-btn icon-btn-sm icon-btn-danger",
            title: protectedRow ? T("mobileBaseProtected") : T("remove"),
            disabled: protectedRow,
            html: E.icons.trash,
            onclick: () => {
              row.removed = true;
              row.el.remove();
            },
          })
        );
        rows.push(row);
        list.appendChild(row.el);
      };
      layouts.forEach(addRow);
      card.appendChild(
        h(
          "div",
          { class: "editor-form" },
          h("p", { class: "muted", text: T("layoutsHint"), style: "margin:0" }),
          list,
          h(
            "div",
            { class: "editor-actions" },
            h("button", { type: "button", class: "btn", text: T("addLayout"), onclick: () => addRow(null) }),
            h("button", { type: "button", class: "btn", text: T("cancel"), onclick: close }),
            h("button", {
              type: "button",
              class: "btn btn-primary",
              text: T("save"),
              onclick: async () => {
                const payload = rows
                  .filter((r) => !r.removed)
                  .map((r) => ({
                    id: r.l ? r.l.id : undefined,
                    name: r.name.value.trim(),
                    breakpoint: parseInt(r.bp.value, 10) || 0,
                    columnCount: parseInt(r.cols.value, 10) || 1,
                  }));
                if (dirty || saveTimer || pending || saving) {
                  try {
                    await saveNow();
                  } catch (_) {
                    return;
                  }
                }
                const resp = await fetch("/api/boards/" + encodeURIComponent(E.boardId) + "/layouts", {
                  method: "PUT",
                  headers: { "Content-Type": "application/json" },
                  body: JSON.stringify({ layouts: payload }),
                });
                if (!resp.ok) {
                  const err = await resp.json().catch(() => ({}));
                  return toast((err.error && err.error.message) || "Request failed", "error");
                }
                store.set(LAYOUT_KEY, null);
                location.reload();
              },
            })
          )
        )
      );
    });
  }

  // ------------------------------------------------------------ item actions
  async function removeItem(id) {
    if (!confirm(T("confirmRemoveItem"))) return;
    items.delete(id);
    await saveAndReload();
  }

  async function duplicateItem(id) {
    const src = items.get(id);
    if (!src) return;
    const p = placement(src);
    const copy = clone(src);
    copy.id = newId();
    copy.p = {};
    delete copy.o;
    copy.p[curLayoutId] = firstFree(src.sectionId, p[2], p[3]).concat(1);
    const baseLayout = layouts.find((l) => l.role === "base") || layouts[0];
    if (baseLayout && baseLayout.id !== curLayoutId) {
      const bw = clamp(p[2], 1, Math.max(1, baseLayout.columnCount || 1));
      const bh = clamp(p[3], 1, MAX_H);
      copy.p[baseLayout.id] = [0, 0, bw, bh, 1];
    }
    items.set(copy.id, copy);
    await saveAndReload();
  }

  // ------------------------------------------------------- drag & resize core
  function gridMetrics(grid) {
    const cs = getComputedStyle(grid);
    const rect = grid.getBoundingClientRect();
    const cols = curCols();
    const gapX = parseFloat(cs.columnGap) || 12;
    const gapY = parseFloat(cs.rowGap) || 12;
    const rowH = parseFloat(cs.gridAutoRows) || ROW_FALLBACK;
    const colW = (rect.width - gapX * (cols - 1)) / cols;
    return { rect, cols, pitchX: colW + gapX, pitchY: rowH + gapY, gapX, gapY };
  }

  let gesture = null;

  function previewSettle(sectionId, pinned, excludeId, skipId) {
    const res = settle(sectionId, pinned, excludeId);
    for (const [id, r] of res) if (id !== skipId) applyView(id, r);
    return res;
  }

  function commitResult(res) {
    for (const [id, r] of res) {
      const it = items.get(id);
      if (it) it.p[curLayoutId] = r.concat(1);
    }
  }

  document.addEventListener("pointerdown", (e) => {
    if (!document.body.classList.contains("edit-mode") || e.button !== 0) return;
    const resizeHandle = e.target.closest("[data-editor-resize]");
    const dragHandle = e.target.closest("[data-editor-drag]");
    if (!resizeHandle && !dragHandle) return;
    const el = (resizeHandle || dragHandle).closest(".grid-item");
    const it = el && items.get(el.dataset.itemId);
    if (!it) return;
    e.preventDefault();
    const handle = resizeHandle || dragHandle;
    try {
      handle.setPointerCapture(e.pointerId);
    } catch (_) {}
    const p = placement(it).slice();
    gesture = {
      type: resizeHandle ? "resize" : "drag",
      it,
      el,
      handle,
      startX: e.clientX,
      startY: e.clientY,
      startRect: el.getBoundingClientRect(),
      startP: p,
      started: false,
      target: it.sectionId,
      placeholder: null,
      result: null,
      lastSection: it.sectionId,
    };
  });

  function beginDrag(g) {
    g.started = true;
    const r = g.startRect;
    g.el.classList.add("is-dragging");
    g.el.style.width = r.width + "px";
    g.el.style.height = r.height + "px";
    g.el.style.left = r.left + "px";
    g.el.style.top = r.top + "px";
    document.body.appendChild(g.el);
    g.placeholder = h("div", { class: "grid-placeholder" });
    const grid = gridOf(g.it.sectionId);
    if (grid) grid.appendChild(g.placeholder);
  }

  function targetGrid(x, y, fallbackSection) {
    let best = null;
    document.querySelectorAll(".board-section .board-grid").forEach((grid) => {
      const r = grid.getBoundingClientRect();
      if (r.width === 0 || r.height === 0) return;
      if (x >= r.left && x <= r.right && y >= r.top - 24 && y <= r.bottom + 24) {
        if (!best || r.height < best.rect.height) best = { grid, rect: r };
      }
    });
    if (best) return best.grid.closest(".board-section").dataset.sectionId;
    return fallbackSection;
  }

  document.addEventListener("pointermove", (e) => {
    const g = gesture;
    if (!g) return;
    const dx = e.clientX - g.startX;
    const dy = e.clientY - g.startY;
    if (!g.started && Math.hypot(dx, dy) < 5) return;

    if (g.type === "drag") {
      if (!g.started) beginDrag(g);
      g.el.style.left = g.startRect.left + dx + "px";
      g.el.style.top = g.startRect.top + dy + "px";
      const sectionId = targetGrid(e.clientX, e.clientY, g.target);
      const grid = gridOf(sectionId);
      if (!grid) return;
      if (g.placeholder.parentElement !== grid) grid.appendChild(g.placeholder);
      document.querySelectorAll(".board-grid.is-drop-target").forEach((n) => n.classList.remove("is-drop-target"));
      grid.classList.add("is-drop-target");
      const m = gridMetrics(grid);
      const w = clamp(g.startP[2], 1, m.cols);
      const x = clamp(Math.round((g.startRect.left + dx - m.rect.left) / m.pitchX), 0, m.cols - w);
      const y = Math.max(0, Math.round((g.startRect.top + dy - m.rect.top) / m.pitchY));
      const pinned = { id: g.it.id, x, y, w, h: g.startP[3] };
      g.target = sectionId;
      g.result = previewSettle(sectionId, pinned, null, g.it.id);
      if (g.lastSection !== sectionId) previewSettle(g.lastSection, null, g.it.id, g.it.id);
      g.lastSection = sectionId;
      if (g.target !== g.it.sectionId) previewSettle(g.it.sectionId, null, g.it.id, g.it.id);
      const r = g.result.get(g.it.id);
      g.placeholder.style.setProperty("--x", r[0]);
      g.placeholder.style.setProperty("--y", r[1]);
      g.placeholder.style.setProperty("--w", r[2]);
      g.placeholder.style.setProperty("--h", r[3]);
    } else {
      if (!g.started) {
        g.started = true;
        g.el.classList.add("is-resizing");
      }
      const grid = g.el.parentElement;
      const m = gridMetrics(grid);
      const [px, py, pw, ph] = g.startP;
      const w = clamp(pw + Math.round(dx / m.pitchX), 1, m.cols - px);
      const hh = clamp(ph + Math.round(dy / m.pitchY), 1, MAX_H);
      g.result = previewSettle(g.it.sectionId, { id: g.it.id, x: px, y: py, w, h: hh }, null, null);
    }
  });

  function endGesture(cancel) {
    const g = gesture;
    gesture = null;
    if (!g) return;
    document.querySelectorAll(".board-grid.is-drop-target").forEach((n) => n.classList.remove("is-drop-target"));
    if (!g.started) return;
    if (g.type === "drag") {
      const el = g.el;
      const grid = gridOf(g.target) || gridOf(g.it.sectionId);
      el.classList.remove("is-dragging");
      ["width", "height", "left", "top"].forEach((k) => el.style.removeProperty(k));
      if (g.placeholder) {
        g.placeholder.replaceWith(el);
        g.placeholder = null;
      } else if (grid) grid.appendChild(el);
      if (!cancel && g.result) {
        const moved = g.target !== g.it.sectionId;
        const sourceId = g.it.sectionId;
        if (moved) changeSection(g.it, g.target);
        claim(g.it);
        commitResult(g.result);
        if (moved) commitResult(settle(sourceId, null, null));
        scheduleSave();
      } else {
        // restore previous positions
        const res = settle(g.it.sectionId, null, null);
        for (const [id, r] of res) applyView(id, r);
        if (grid && gridOf(g.it.sectionId) !== el.parentElement) gridOf(g.it.sectionId).appendChild(el);
      }
    } else {
      g.el.classList.remove("is-resizing");
      if (!cancel && g.result) {
        claim(g.it);
        commitResult(g.result);
        scheduleSave();
      }
    }
    applyLayout();
  }

  document.addEventListener("pointerup", () => endGesture(false));
  document.addEventListener("pointercancel", () => endGesture(true));

  // ------------------------------------------------------ action delegation
  document.addEventListener("click", (e) => {
    const btn = e.target.closest("[data-editor-action]");
    if (!btn) return;
    const action = btn.dataset.editorAction;
    const itemId = btn.dataset.itemId;
    const sectionId = btn.dataset.sectionId;
    switch (action) {
      case "add-widget":
        return openPicker(sortedSections()[0].id);
      case "add-section":
        return openSectionEditor(null);
      case "layouts":
        return openLayoutsEditor();
      case "exit":
        return exitEdit();
      case "edit-item":
        return openItemEditor({ item: items.get(itemId) });
      case "duplicate-item":
        return duplicateItem(itemId);
      case "remove-item":
        return removeItem(itemId);
      case "section-up":
        return moveSection(sectionId, -1);
      case "section-down":
        return moveSection(sectionId, 1);
      case "section-edit":
        return openSectionEditor(sections.find((s) => s.id === sectionId));
      case "section-remove":
        return removeSection(sectionId);
    }
  });

  // Make sure unsaved moves are flushed when leaving the page.
  window.addEventListener("beforeunload", () => {
    if (saveTimer || pending || dirty) {
      clearTimeout(saveTimer);
      saveTimer = null;
      try {
        fetch("/api/boards/" + encodeURIComponent(E.boardId) + "/layout", {
          method: "PUT",
          headers: { "Content-Type": "application/json" },
          body: JSON.stringify(buildPayload()),
          keepalive: true,
        });
      } catch (_) {}
    }
  });

  // ------------------------------------------------------------------- boot
  curLayoutId = pickInitialLayout();
  if (store.get(EDIT_KEY) === E.boardId) enterEdit();
})();
