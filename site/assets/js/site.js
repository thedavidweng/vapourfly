(() => {
  "use strict";

  const $ = (sel, root = document) => root.querySelector(sel);
  const $$ = (sel, root = document) => [...root.querySelectorAll(sel)];

  // Store nav tucks away while scrolling down and returns on scroll up,
  // leaving only the product bar pinned.
  const sn = $("#sn");
  if (sn) {
    const header = $(".gh");
    let lastY = window.scrollY;
    const update = () => {
      const y = window.scrollY;
      const pinned = y > header.offsetHeight;
      if (!pinned || y < lastY - 4) sn.classList.remove("is-tucked");
      else if (y > lastY + 4) sn.classList.add("is-tucked");
      lastY = y;
    };
    window.addEventListener("scroll", update, { passive: true });
  }

  // Dropdowns open on hover via CSS; this adds click and keyboard support.
  $$(".sn__tab--drop").forEach((tab) => {
    const btn = $("button", tab);
    btn.addEventListener("click", () => {
      const open = !tab.classList.contains("is-open");
      $$(".sn__tab--drop.is-open").forEach((t) => {
        t.classList.remove("is-open");
        $("button", t).setAttribute("aria-expanded", "false");
      });
      tab.classList.toggle("is-open", open);
      btn.setAttribute("aria-expanded", String(open));
    });
  });
  document.addEventListener("click", (e) => {
    if (e.target.closest(".sn__tab--drop")) return;
    $$(".sn__tab--drop.is-open").forEach((t) => {
      t.classList.remove("is-open");
      $("button", t).setAttribute("aria-expanded", "false");
    });
  });
  document.addEventListener("keydown", (e) => {
    if (e.key !== "Escape") return;
    $$(".sn__tab--drop.is-open").forEach((t) => {
      t.classList.remove("is-open");
      $("button", t).setAttribute("aria-expanded", "false");
      $("button", t).focus();
    });
    closeMenu();
  });

  // Mobile menu
  const menuBtn = $(".gh__menu");
  const menu = $("#mobile-menu");
  function closeMenu() {
    if (!menu || menu.hidden) return;
    menu.hidden = true;
    menuBtn.setAttribute("aria-expanded", "false");
    document.body.style.overflow = "";
  }
  if (menuBtn && menu) {
    menuBtn.addEventListener("click", () => {
      const open = menu.hidden;
      menu.hidden = !open;
      menuBtn.setAttribute("aria-expanded", String(open));
      document.body.style.overflow = open ? "hidden" : "";
    });
    $$("a", menu).forEach((a) => a.addEventListener("click", closeMenu));
  }

  // Docs search goes to GitHub code search scoped to this repo's docs.
  const form = $(".sn__search");
  if (form) {
    form.addEventListener("submit", (e) => {
      const q = $(".sn__q", form);
      const term = q.value.trim();
      e.preventDefault();
      const url = new URL(form.action);
      url.searchParams.set("q", q.dataset.prefix + term);
      url.searchParams.set("type", "code");
      window.location.href = url.toString();
    });
  }

  // Moods carousel
  const rail = $(".rail");
  if (rail) {
    const track = $(".rail__track", rail);
    const prev = $(".rail__arrow--prev", rail);
    const next = $(".rail__arrow--next", rail);
    const dots = $(".rail__dots", rail);
    const cards = $$(".mood", track);

    const perPage = () => {
      const w = cards[0].getBoundingClientRect().width;
      const gap = parseFloat(getComputedStyle(track).columnGap) || 0;
      const pad = parseFloat(getComputedStyle(track).paddingLeft) || 0;
      return Math.max(1, Math.round((track.clientWidth - 2 * pad + gap) / (w + gap)));
    };
    const pages = () => Math.ceil(cards.length / perPage());
    const step = () => {
      const w = cards[0].getBoundingClientRect().width;
      const gap = parseFloat(getComputedStyle(track).columnGap) || 0;
      return perPage() * (w + gap);
    };

    const sizeArrows = () => {
      const art = $(".mood__art", track);
      if (art) rail.style.setProperty("--art-h", art.getBoundingClientRect().height + "px");
      [prev, next].forEach((b) => { b.style.height = art.getBoundingClientRect().height + "px"; });
    };

    const renderDots = () => {
      const n = pages();
      if (dots.childElementCount !== n) {
        dots.replaceChildren(...Array.from({ length: n }, () => document.createElement("i")));
      }
      sync();
    };

    const sync = () => {
      const max = track.scrollWidth - track.clientWidth;
      prev.disabled = track.scrollLeft <= 2;
      next.disabled = track.scrollLeft >= max - 2;
      const n = dots.childElementCount;
      const idx = max <= 0 ? 0 : Math.round((track.scrollLeft / max) * (n - 1));
      [...dots.children].forEach((d, i) => d.classList.toggle("is-on", i === idx));
    };

    prev.addEventListener("click", () => track.scrollBy({ left: -step(), behavior: "smooth" }));
    next.addEventListener("click", () => track.scrollBy({ left: step(), behavior: "smooth" }));
    track.addEventListener("scroll", () => requestAnimationFrame(sync), { passive: true });
    window.addEventListener("resize", () => { sizeArrows(); renderDots(); });
    sizeArrows();
    renderDots();
  }

  // Platform picker
  const SKUS = {
    mac: {
      name: "Vapourfly for macOS",
      list: [
        "Desktop app and <code>vapourfly</code> command line in one download",
        "Apple Silicon build; Intel Macs build from source",
        "Reads your local Steam files. No account, no sign-in",
        "Open source, AGPL-3.0",
      ],
      cta: "Download for macOS",
      href: "vapourfly-macos-aarch64.tar.gz",
      fine: "Builds are not notarized. The first time, right-click the app and choose <b>Open</b>.",
    },
    deck: {
      name: "Vapourfly for Steam Deck",
      list: [
        "Runs in Desktop Mode and fullscreen in Game Mode",
        "Full controller support and the SteamOS on-screen keyboard",
        "One installer adds Vapourfly to your Steam library",
        "Open source, AGPL-3.0",
      ],
      cta: "Download for SteamOS",
      href: "vapourfly-linux-x86_64.tar.gz",
      fine: "In Desktop Mode, extract the archive and run <code>./install-steamos.sh</code>. SteamOS already has every library Vapourfly needs.",
    },
    linux: {
      name: "Vapourfly for Linux",
      list: [
        "Desktop app and <code>vapourfly</code> command line in one download",
        "x86_64, X11 and Wayland",
        "Reads your local Steam files. No account, no sign-in",
        "Open source, AGPL-3.0",
      ],
      cta: "Download for Linux",
      href: "vapourfly-linux-x86_64.tar.gz",
      fine: "Needs common desktop libraries such as libxkbcommon, libwayland and libudev. The README lists the exact packages.",
    },
    win: {
      name: "Vapourfly for Windows",
      list: [
        "Desktop app and <code>vapourfly</code> command line in one download",
        "Windows 10 and 11, x86_64",
        "Reads your local Steam files. No account, no sign-in",
        "Open source, AGPL-3.0",
      ],
      cta: "Download for Windows",
      href: "vapourfly-windows-x86_64.zip",
      fine: "Extract the zip and run <code>vapourfly-gui.exe</code>. Put <code>vapourfly.exe</code> on your PATH to use the command line.",
    },
  };
  const RELEASE = "https://github.com/thedavidweng/vapourfly/releases/latest/download/";
  const opts = $$(".sku__opt");
  const selectSku = (key, focus) => {
    const sku = SKUS[key];
    if (!sku) return;
    opts.forEach((o) => {
      const on = o.dataset.sku === key;
      o.classList.toggle("is-selected", on);
      o.setAttribute("aria-checked", String(on));
      o.tabIndex = on ? 0 : -1;
      if (on && focus) o.focus();
    });
    $("[data-sku-name]").textContent = sku.name;
    $("[data-sku-list]").innerHTML = sku.list.map((li) => `<li>${li}</li>`).join("");
    const cta = $("[data-sku-cta]");
    cta.textContent = sku.cta;
    cta.href = RELEASE + sku.href;
    $("[data-sku-fine]").innerHTML = sku.fine;
  };
  opts.forEach((o, i) => {
    o.addEventListener("click", () => selectSku(o.dataset.sku));
    o.addEventListener("keydown", (e) => {
      const d = { ArrowRight: 1, ArrowDown: 1, ArrowLeft: -1, ArrowUp: -1 }[e.key];
      if (!d) return;
      e.preventDefault();
      selectSku(opts[(i + d + opts.length) % opts.length].dataset.sku, true);
    });
  });
  if (opts.length) {
    const ua = navigator.userAgent;
    const guess = /SteamOS|Steam Deck|Valve Steam/i.test(ua) ? "deck"
      : /Windows/i.test(ua) ? "win"
      : /Linux|X11/i.test(ua) && !/Android/i.test(ua) ? "linux"
      : "mac";
    selectSku(guess);
  }

  // Tech spec tabs
  const tabs = $$('.specs__tabs [role="tab"]');
  const showTab = (tab, focus) => {
    tabs.forEach((t) => {
      const on = t === tab;
      t.setAttribute("aria-selected", String(on));
      t.tabIndex = on ? 0 : -1;
      document.getElementById(t.getAttribute("aria-controls")).hidden = !on;
    });
    if (focus) tab.focus();
  };
  tabs.forEach((t, i) => {
    t.addEventListener("click", () => showTab(t));
    t.addEventListener("keydown", (e) => {
      const d = { ArrowRight: 1, ArrowLeft: -1 }[e.key];
      if (!d) return;
      e.preventDefault();
      showTab(tabs[(i + d + tabs.length) % tabs.length], true);
    });
  });
})();
