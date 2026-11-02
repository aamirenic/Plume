/* PLUME site behaviour: navigation, scroll reveals, ribbon demo, lightbox, copy. */
(() => {
  "use strict";

  const reduceMotion = window.matchMedia("(prefers-reduced-motion: reduce)").matches;

  /* ---------------------------------------------------------------------
     Header + mobile navigation
     --------------------------------------------------------------------- */

  const header = document.querySelector("[data-header]");
  const nav = document.getElementById("site-nav");
  const menuToggle = document.querySelector("[data-menu-toggle]");

  const setNavOpen = (open) => {
    if (!header || !menuToggle) return;
    header.classList.toggle("nav-open", open);
    menuToggle.setAttribute("aria-expanded", String(open));
  };

  if (header) {
    const onScroll = () => header.classList.toggle("is-scrolled", window.scrollY > 24);
    onScroll();
    window.addEventListener("scroll", onScroll, { passive: true });
  }

  if (menuToggle && nav) {
    menuToggle.addEventListener("click", () => {
      setNavOpen(menuToggle.getAttribute("aria-expanded") !== "true");
    });

    nav.addEventListener("click", (event) => {
      if (event.target.closest("a")) setNavOpen(false);
    });

    document.addEventListener("keydown", (event) => {
      if (event.key === "Escape" && header?.classList.contains("nav-open")) {
        setNavOpen(false);
        menuToggle.focus();
      }
    });

    window.addEventListener("resize", () => {
      if (window.innerWidth > 880) setNavOpen(false);
    });
  }

  /* ---------------------------------------------------------------------
     Scroll reveal
     --------------------------------------------------------------------- */

  const revealTargets = [
    ".section-head",
    ".studio-copy",
    ".demo-panel",
    ".card",
    ".gallery-head",
    ".scene",
    ".interface-copy",
    ".interface-shot",
    ".android-copy",
    ".phone-stage",
    ".download-head",
    ".download-card",
    ".download-note",
  ];

  const targets = [...document.querySelectorAll(revealTargets.join(","))];

  if (!reduceMotion && "IntersectionObserver" in window) {
    targets.forEach((el) => el.classList.add("reveal"));

    const observer = new IntersectionObserver(
      (entries) => {
        entries.forEach((entry) => {
          if (!entry.isIntersecting) return;
          const el = entry.target;
          const siblings = [...(el.parentElement?.children ?? [])].filter((node) =>
            node.classList.contains("reveal")
          );
          const index = Math.max(0, siblings.indexOf(el));
          el.style.transitionDelay = `${Math.min(index * 55, 330)}ms`;
          el.classList.add("is-visible");
          observer.unobserve(el);
        });
      },
      { threshold: 0.12, rootMargin: "0px 0px -8% 0px" }
    );

    targets.forEach((el) => observer.observe(el));
  }

  /* ---------------------------------------------------------------------
     Ribbon demo canvas
     --------------------------------------------------------------------- */

  const canvas = document.getElementById("ribbon-demo");
  const resetButton = document.querySelector("[data-demo-reset]");

  if (canvas && canvas.getContext) {
    const ctx = canvas.getContext("2d");
    const PALETTE = ["#75ffd6", "#f3f2ed", "#ff8fc9", "#d8ff3e"];
    const strokes = [];
    let active = null;
    let colourIndex = 0;
    let lastTime = 0;
    let dpr = 1;
    let width = 900;
    let height = 620;

    const resize = () => {
      const rect = canvas.getBoundingClientRect();
      if (!rect.width) return;
      dpr = Math.min(window.devicePixelRatio || 1, 2);
      width = rect.width;
      height = rect.height;
      canvas.width = Math.round(width * dpr);
      canvas.height = Math.round(height * dpr);
      ctx.setTransform(dpr, 0, 0, dpr, 0, 0);
      paint();
    };

    const sampleStroke = () => {
      const points = [];
      const start = { x: width * 0.14, y: height * 0.66 };
      for (let i = 0; i <= 120; i += 1) {
        const t = i / 120;
        points.push({
          x: start.x + t * width * 0.72,
          y:
            start.y -
            Math.sin(t * Math.PI * 1.35) * height * 0.3 -
            Math.sin(t * Math.PI * 3.1) * height * 0.045,
          w: 2.2 + Math.sin(t * Math.PI) * 9.5,
        });
      }
      strokes.push({ colour: PALETTE[0], points, born: performance.now() });
      strokes.push({
        colour: PALETTE[2],
        points: points.map((p, i) => ({
          x: p.x + width * 0.02,
          y: p.y + height * 0.1 + Math.sin(i * 0.22) * 6,
          w: p.w * 0.32,
        })),
        born: performance.now(),
      });
    };

    const grid = () => {
      ctx.save();
      ctx.strokeStyle = "rgba(243, 242, 237, 0.07)";
      ctx.lineWidth = 1;
      const horizon = height * 0.52;
      for (let i = 0; i <= 12; i += 1) {
        const x = (i / 12) * width;
        ctx.beginPath();
        ctx.moveTo(x, height);
        ctx.lineTo(width * 0.5 + (x - width * 0.5) * 0.32, horizon);
        ctx.stroke();
      }
      for (let i = 1; i <= 6; i += 1) {
        const t = i / 6;
        const y = horizon + Math.pow(t, 1.8) * (height - horizon);
        ctx.beginPath();
        ctx.moveTo(0, y);
        ctx.lineTo(width, y);
        ctx.stroke();
      }
      ctx.restore();
    };

    const paintStroke = (stroke) => {
      const points = stroke.points;
      if (points.length < 2) return;
      ctx.save();
      ctx.lineCap = "round";
      ctx.lineJoin = "round";

      // Soft halo pass.
      ctx.globalCompositeOperation = "lighter";
      ctx.globalAlpha = 0.12;
      ctx.strokeStyle = stroke.colour;
      for (let i = 1; i < points.length; i += 1) {
        const a = points[i - 1];
        const b = points[i];
        ctx.beginPath();
        ctx.moveTo(a.x, a.y);
        ctx.lineTo(b.x, b.y);
        ctx.lineWidth = Math.max(2, b.w * 2.6);
        ctx.stroke();
      }

      // Ribbon body.
      ctx.globalAlpha = 0.94;
      ctx.globalCompositeOperation = "source-over";
      ctx.strokeStyle = stroke.colour;
      for (let i = 1; i < points.length; i += 1) {
        const a = points[i - 1];
        const b = points[i];
        ctx.beginPath();
        ctx.moveTo(a.x, a.y);
        ctx.lineTo(b.x, b.y);
        ctx.lineWidth = Math.max(0.8, (a.w + b.w) * 0.5);
        ctx.stroke();
      }

      // Specular core.
      ctx.globalAlpha = 0.5;
      ctx.strokeStyle = "#ffffff";
      ctx.lineWidth = 0.9;
      ctx.beginPath();
      ctx.moveTo(points[0].x, points[0].y);
      for (let i = 1; i < points.length; i += 1) ctx.lineTo(points[i].x, points[i].y);
      ctx.stroke();
      ctx.restore();
    };

    const paint = () => {
      ctx.save();
      ctx.setTransform(dpr, 0, 0, dpr, 0, 0);
      ctx.clearRect(0, 0, width, height);
      const bg = ctx.createLinearGradient(0, 0, 0, height);
      bg.addColorStop(0, "#07080a");
      bg.addColorStop(1, "#0b0d11");
      ctx.fillStyle = bg;
      ctx.fillRect(0, 0, width, height);
      grid();
      strokes.forEach(paintStroke);
      ctx.restore();
    };

    const pointFromEvent = (event) => {
      const rect = canvas.getBoundingClientRect();
      return { x: event.clientX - rect.left, y: event.clientY - rect.top };
    };

    const begin = (event) => {
      if (event.button !== undefined && event.button !== 0) return;
      event.preventDefault();
      canvas.setPointerCapture?.(event.pointerId);
      const p = pointFromEvent(event);
      colourIndex = (colourIndex + 1) % PALETTE.length;
      active = { colour: PALETTE[colourIndex], points: [{ ...p, w: 4 }], born: performance.now() };
      strokes.push(active);
      lastTime = performance.now();
      paint();
    };

    const move = (event) => {
      if (!active) return;
      event.preventDefault();
      const now = performance.now();
      const p = pointFromEvent(event);
      const last = active.points[active.points.length - 1];
      const dx = p.x - last.x;
      const dy = p.y - last.y;
      const distance = Math.hypot(dx, dy);
      if (distance < 1.6) return;
      const dt = Math.max(8, now - lastTime);
      const speed = distance / dt; // px per ms
      const w = Math.max(0.9, 11 - speed * 13);
      active.points.push({ ...p, w });
      lastTime = now;
      if (active.points.length > 900) active.points.shift();
      paint();
    };

    const end = (event) => {
      if (!active) return;
      event.preventDefault();
      active = null;
      paint();
    };

    canvas.addEventListener("pointerdown", begin);
    canvas.addEventListener("pointermove", move);
    canvas.addEventListener("pointerup", end);
    canvas.addEventListener("pointercancel", end);
    canvas.addEventListener("pointerleave", (event) => {
      if (event.buttons === 0) end(event);
    });

    resetButton?.addEventListener("click", () => {
      strokes.length = 0;
      active = null;
      paint();
      resetButton.blur();
    });

    if ("ResizeObserver" in window) {
      new ResizeObserver(resize).observe(canvas);
    } else {
      window.addEventListener("resize", resize);
    }

    resize();
    sampleStroke();
    paint();
  }

  /* ---------------------------------------------------------------------
     Scene lightbox
     --------------------------------------------------------------------- */

  const dialog = document.getElementById("lightbox");
  const sceneButtons = [...document.querySelectorAll("[data-lightbox]")];

  if (dialog && sceneButtons.length) {
    const image = dialog.querySelector("[data-lightbox-img]");
    const title = dialog.querySelector("#lightbox-title");
    const meta = dialog.querySelector("[data-lightbox-meta]");
    let current = 0;
    let opener = null;

    const render = (index) => {
      current = (index + sceneButtons.length) % sceneButtons.length;
      const button = sceneButtons[current];
      const source = button.querySelector("img");
      image.src = button.dataset.src || source?.src || "";
      image.alt = button.dataset.alt || source?.alt || "";
      title.textContent = button.dataset.title || "Scene";
      meta.textContent = button.dataset.meta || "Render";
    };

    const open = (index, button) => {
      opener = button;
      render(index);
      if (typeof dialog.showModal === "function") {
        dialog.showModal();
      } else {
        dialog.setAttribute("open", "");
      }
      document.body.classList.add("lightbox-open");
      dialog.querySelector("[data-lightbox-close]")?.focus();
    };

    const close = () => {
      if (typeof dialog.close === "function" && dialog.open) {
        dialog.close();
      } else {
        dialog.removeAttribute("open");
      }
      document.body.classList.remove("lightbox-open");
      opener?.focus();
    };

    sceneButtons.forEach((button, index) => {
      button.addEventListener("click", () => open(index, button));
    });

    dialog.querySelector("[data-lightbox-close]")?.addEventListener("click", close);
    dialog.querySelector("[data-lightbox-prev]")?.addEventListener("click", () => render(current - 1));
    dialog.querySelector("[data-lightbox-next]")?.addEventListener("click", () => render(current + 1));

    dialog.addEventListener("click", (event) => {
      if (event.target === dialog) close();
    });

    dialog.addEventListener("cancel", (event) => {
      event.preventDefault();
      close();
    });

    document.addEventListener("keydown", (event) => {
      if (!dialog.open) return;
      if (event.key === "ArrowLeft") {
        event.preventDefault();
        render(current - 1);
      } else if (event.key === "ArrowRight") {
        event.preventDefault();
        render(current + 1);
      }
    });
  }

  /* ---------------------------------------------------------------------
     Checksum copy
     --------------------------------------------------------------------- */

  document.querySelectorAll("[data-copy]").forEach((button) => {
    const original = button.innerHTML;
    let timer = 0;

    const flash = (label) => {
      button.innerHTML = `${label} <span aria-hidden="true">✓</span>`;
      window.clearTimeout(timer);
      timer = window.setTimeout(() => {
        button.innerHTML = original;
      }, 1600);
    };

    button.addEventListener("click", async () => {
      const value = button.dataset.copy || "";
      try {
        await navigator.clipboard.writeText(value);
        flash("COPIED");
      } catch {
        const area = document.createElement("textarea");
        area.value = value;
        area.setAttribute("readonly", "");
        area.style.position = "fixed";
        area.style.opacity = "0";
        document.body.appendChild(area);
        area.select();
        try {
          document.execCommand("copy");
          flash("COPIED");
        } catch {
          flash("SELECT + COPY");
        }
        area.remove();
      }
    });
  });
})();
