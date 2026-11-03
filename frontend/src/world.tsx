import { useEffect, useRef } from "react";

export const worldLayers = ["sky", "airship", "far", "islands", "harbour", "sea"] as const;
const depths = [0.025, 0.1, 0.18, 0.29, 0.44, 0.62];

export function World({ compact = false }: { compact?: boolean }) {
  const art = useRef<HTMLDivElement>(null);

  useEffect(() => {
    const root = art.current,
      region = root?.parentElement;
    if (!root || !region) return;
    const layers = [...root.children] as HTMLElement[],
      motion = media("(prefers-reduced-motion: reduce)"),
      reducedData = media("(prefers-reduced-data: reduce)"),
      finePointer = media("(pointer: fine)"),
      connection = (navigator as DataNavigator).connection;
    let frame = 0,
      intersecting = true,
      targetX = 0,
      targetY = 0,
      currentX = 0,
      currentY = 0;

    const disabled = () => motion.matches || reducedData.matches || Boolean(connection?.saveData);
    const reset = () => {
      targetX = 0;
      targetY = 0;
      currentX = 0;
      currentY = 0;
      for (const layer of layers) layer.style.removeProperty("transform");
    };
    const stop = () => {
      cancelAnimationFrame(frame);
      frame = 0;
    };
    const tick = (time: number) => {
      if (document.hidden || !intersecting || disabled()) {
        frame = 0;
        return;
      }
      currentX += (targetX - currentX) * 0.075;
      currentY += (targetY - currentY) * 0.075;
      const settled = Math.abs(targetX - currentX) < 0.08 && Math.abs(targetY - currentY) < 0.08;
      layers.forEach((layer, index) => {
        const depth = depths[index],
          ambient = finePointer.matches ? Math.sin(time / (11000 - index * 900)) * index * 0.45 : 0,
          x = currentX * depth + ambient,
          y = currentY * depth * 0.65 - ambient * 0.4;
        layer.style.transform = `translate3d(${x.toFixed(2)}px, ${y.toFixed(2)}px, 0) scale(1.045)`;
      });
      if (!finePointer.matches && settled) frame = 0;
      else frame = requestAnimationFrame(tick);
    };
    const start = () => {
      if (finePointer.matches && !frame && !document.hidden && intersecting && !disabled()) frame = requestAnimationFrame(tick);
    };
    const move = (event: PointerEvent) => {
      if (!finePointer.matches || event.pointerType === "touch" || disabled()) return;
      const bounds = region.getBoundingClientRect();
      targetX = ((event.clientX - bounds.left) / bounds.width - 0.5) * 42;
      targetY = ((event.clientY - bounds.top) / bounds.height - 0.5) * 24;
      start();
    };
    const settle = () => {
      targetX = 0;
      targetY = 0;
      start();
    };
    const visibility = () => document.hidden ? stop() : start();
    const preference = () => {
      root.dataset.depth = disabled() ? "still" : finePointer.matches ? "pointer" : "scroll";
      if (disabled()) {
        stop();
        reset();
      } else if (!finePointer.matches) {
        stop();
        reset();
      } else {
        start();
      }
    };
    const observer = typeof IntersectionObserver === "function"
      ? new IntersectionObserver(([entry]) => {
          intersecting = entry?.isIntersecting ?? true;
          if (intersecting) {
            start();
          } else stop();
        }, { rootMargin: "80px" })
      : null;

    region.addEventListener("pointermove", move, { passive: true });
    region.addEventListener("pointerleave", settle);
    document.addEventListener("visibilitychange", visibility);
    motion.addEventListener("change", preference);
    reducedData.addEventListener("change", preference);
    finePointer.addEventListener("change", preference);
    connection?.addEventListener?.("change", preference);
    observer?.observe(region);
    preference();
    return () => {
      stop();
      observer?.disconnect();
      region.removeEventListener("pointermove", move);
      region.removeEventListener("pointerleave", settle);
      document.removeEventListener("visibilitychange", visibility);
      motion.removeEventListener("change", preference);
      reducedData.removeEventListener("change", preference);
      finePointer.removeEventListener("change", preference);
      connection?.removeEventListener?.("change", preference);
      delete root.dataset.depth;
      reset();
    };
  }, []);

  return (
    <div ref={art} className={`world${compact ? " world-compact" : ""}`} aria-hidden="true">
      {worldLayers.map((layer) => <span className={`world-${layer}`} key={layer} />)}
    </div>
  );
}

type DataNavigator = Navigator & {
  connection?: {
    saveData?: boolean;
    addEventListener?: (type: "change", listener: () => void) => void;
    removeEventListener?: (type: "change", listener: () => void) => void;
  };
};

function media(query: string): MediaQueryList {
  if (typeof matchMedia === "function") return matchMedia(query);
  return {
    matches: false,
    media: query,
    onchange: null,
    addEventListener() {},
    removeEventListener() {},
    addListener() {},
    removeListener() {},
    dispatchEvent: () => false,
  };
}
