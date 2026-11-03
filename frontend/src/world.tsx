import { useEffect, useRef, type CSSProperties } from "react";
import { homeCamera, miniById, type Mini } from "./minis";

export const worldLayers = ["sky", "mist", "far", "islands", "crew", "foreground"] as const;

export function World({ compact = false, scene = "home" }: { compact?: boolean; scene?: "home" | Mini }) {
  const art = useRef<HTMLDivElement>(null);
  const camera = scene === "home" ? homeCamera : miniById[scene].camera;

  useEffect(() => {
    let dispose: (() => void) | undefined,
      active = true;
    void import("./depth").then(({ watchDepth }) => {
      if (active && art.current) dispose = watchDepth(art.current);
    });
    return () => {
      active = false;
      dispose?.();
    };
  }, []);

  return (
    <div
      ref={art}
      className={`world${compact ? " world-compact" : ""}`}
      data-scene={scene}
      style={{
        "--camera-x": `${camera.x}px`,
        "--camera-y": `${camera.y}px`,
        "--camera-scale": camera.scale,
      } as CSSProperties}
      aria-hidden="true"
    >
      {worldLayers.map((layer) => <span className={`world-${layer}`} key={layer} />)}
    </div>
  );
}
