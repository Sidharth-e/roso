import { useEffect, useRef } from "react";
import type { TaskPhase } from "../types";

export type OrbMode = "idle" | "listening" | "thinking" | "acting" | "paused" | "success" | "error";

interface OrbPalette {
  core: [number, number, number];
  a: [number, number, number];
  b: [number, number, number];
  c: [number, number, number];
  speed: number;
  wobble: number;
}

const PALETTES: Record<OrbMode, OrbPalette> = {
  idle: { core: [190, 250, 255], a: [0, 200, 255], b: [40, 110, 255], c: [120, 80, 255], speed: 0.55, wobble: 0.55 },
  listening: { core: [210, 255, 255], a: [0, 225, 255], b: [0, 150, 255], c: [140, 90, 255], speed: 0.9, wobble: 0.8 },
  thinking: { core: [235, 220, 255], a: [150, 90, 255], b: [0, 190, 255], c: [255, 80, 200], speed: 1.35, wobble: 1.0 },
  acting: { core: [210, 255, 255], a: [0, 240, 255], b: [0, 130, 255], c: [90, 255, 220], speed: 1.6, wobble: 1.1 },
  paused: { core: [255, 240, 210], a: [255, 170, 40], b: [255, 120, 60], c: [200, 120, 255], speed: 0.3, wobble: 0.35 },
  success: { core: [215, 255, 235], a: [40, 230, 150], b: [0, 190, 255], c: [120, 255, 200], speed: 0.7, wobble: 0.6 },
  error: { core: [255, 220, 220], a: [255, 70, 90], b: [255, 120, 60], c: [190, 60, 255], speed: 0.8, wobble: 0.7 },
};

export function orbModeForPhase(phase: TaskPhase | undefined): OrbMode {
  switch (phase) {
    case "classifying":
    case "planning":
    case "verifying":
    case "recovering":
      return "thinking";
    case "running":
      return "acting";
    case "paused":
    case "awaiting_confirmation":
      return "paused";
    case "completed":
      return "success";
    case "failed":
      return "error";
    default:
      return "idle";
  }
}

interface AiOrbProps {
  size: number;
  mode: OrbMode;
  /** 0..1 extra excitation (e.g. while the user types). */
  energy?: number;
  /** Draw the outer HUD rings. */
  rings?: boolean;
  className?: string;
}

function lerp(a: number, b: number, t: number) {
  return a + (b - a) * t;
}

function rgba(c: [number, number, number], alpha: number) {
  return `rgba(${c[0] | 0}, ${c[1] | 0}, ${c[2] | 0}, ${alpha})`;
}

/**
 * Canvas-rendered morphing plasma orb. Several noise-deformed blobs are composited
 * additively around a hot core, with optional rotating HUD rings.
 */
export function AiOrb({ size, mode, energy = 0, rings = true, className = "" }: AiOrbProps) {
  const canvasRef = useRef<HTMLCanvasElement | null>(null);
  const targetRef = useRef({ mode, energy });
  targetRef.current = { mode, energy };

  useEffect(() => {
    const canvas = canvasRef.current;
    if (!canvas) {
      return;
    }
    const ctx = canvas.getContext("2d");
    if (!ctx) {
      return;
    }
    const dpr = Math.min(window.devicePixelRatio || 1, 2);
    canvas.width = Math.round(size * dpr);
    canvas.height = Math.round(size * dpr);
    ctx.setTransform(dpr, 0, 0, dpr, 0, 0);

    const reduceMotion =
      typeof window.matchMedia === "function" &&
      window.matchMedia("(prefers-reduced-motion: reduce)").matches;

    // Smoothed, animatable copy of the palette so mode changes blend instead of snapping.
    const start = PALETTES[targetRef.current.mode];
    const live = {
      core: [...start.core] as [number, number, number],
      a: [...start.a] as [number, number, number],
      b: [...start.b] as [number, number, number],
      c: [...start.c] as [number, number, number],
      speed: start.speed,
      wobble: start.wobble,
      energy: 0,
    };

    let raf = 0;
    let t = 0;
    let last = performance.now();
    const cx = size / 2;
    const cy = size / 2;
    const base = size * (rings ? 0.27 : 0.34);

    function blendColor(dst: [number, number, number], src: [number, number, number], k: number) {
      dst[0] = lerp(dst[0], src[0], k);
      dst[1] = lerp(dst[1], src[1], k);
      dst[2] = lerp(dst[2], src[2], k);
    }

    function blobPath(radius: number, seed: number, amp: number) {
      const points = 64;
      ctx!.beginPath();
      for (let i = 0; i <= points; i++) {
        const ang = (i / points) * Math.PI * 2;
        const n =
          Math.sin(ang * 3 + t * 1.1 + seed) * 0.5 +
          Math.sin(ang * 5 - t * 1.7 + seed * 2.3) * 0.3 +
          Math.sin(ang * 2 + t * 0.6 + seed * 0.7) * 0.4;
        const r = radius * (1 + n * amp);
        const x = cx + Math.cos(ang) * r;
        const y = cy + Math.sin(ang) * r;
        if (i === 0) {
          ctx!.moveTo(x, y);
        } else {
          ctx!.lineTo(x, y);
        }
      }
      ctx!.closePath();
    }

    function drawRings() {
      const r1 = size * 0.44;
      const r2 = size * 0.4;
      const r3 = size * 0.475;
      ctx!.lineCap = "round";

      // Segmented outer ring
      ctx!.save();
      ctx!.translate(cx, cy);
      ctx!.rotate(t * 0.18);
      ctx!.strokeStyle = rgba(live.a, 0.45);
      ctx!.lineWidth = 1.5;
      for (let i = 0; i < 3; i++) {
        const s = (i / 3) * Math.PI * 2;
        ctx!.beginPath();
        ctx!.arc(0, 0, r1, s, s + Math.PI * 0.42);
        ctx!.stroke();
      }
      ctx!.restore();

      // Counter-rotating inner ring
      ctx!.save();
      ctx!.translate(cx, cy);
      ctx!.rotate(-t * 0.32);
      ctx!.strokeStyle = rgba(live.b, 0.35);
      ctx!.lineWidth = 1;
      ctx!.setLineDash([2, 6]);
      ctx!.beginPath();
      ctx!.arc(0, 0, r2, 0, Math.PI * 2);
      ctx!.stroke();
      ctx!.setLineDash([]);
      ctx!.restore();

      // Tick marks
      ctx!.save();
      ctx!.translate(cx, cy);
      ctx!.rotate(t * 0.07);
      ctx!.strokeStyle = rgba(live.a, 0.25);
      ctx!.lineWidth = 1;
      const ticks = 72;
      for (let i = 0; i < ticks; i++) {
        const ang = (i / ticks) * Math.PI * 2;
        const len = i % 6 === 0 ? 6 : 2.5;
        ctx!.beginPath();
        ctx!.moveTo(Math.cos(ang) * r3, Math.sin(ang) * r3);
        ctx!.lineTo(Math.cos(ang) * (r3 - len), Math.sin(ang) * (r3 - len));
        ctx!.stroke();
      }
      ctx!.restore();

      // Scanning arc
      ctx!.save();
      ctx!.translate(cx, cy);
      ctx!.rotate(t * (0.9 + live.speed * 0.6));
      const grad = ctx!.createLinearGradient(-r1, 0, r1, 0);
      grad.addColorStop(0, rgba(live.core, 0));
      grad.addColorStop(1, rgba(live.core, 0.9));
      ctx!.strokeStyle = grad;
      ctx!.lineWidth = 2;
      ctx!.beginPath();
      ctx!.arc(0, 0, r1 + 4, 0, Math.PI * 0.25);
      ctx!.stroke();
      ctx!.restore();
    }

    function frame(now: number) {
      const dt = Math.min(0.05, (now - last) / 1000);
      last = now;

      const target = PALETTES[targetRef.current.mode];
      const k = Math.min(1, dt * 3);
      blendColor(live.core, target.core, k);
      blendColor(live.a, target.a, k);
      blendColor(live.b, target.b, k);
      blendColor(live.c, target.c, k);
      live.speed = lerp(live.speed, target.speed, k);
      live.wobble = lerp(live.wobble, target.wobble, k);
      live.energy = lerp(live.energy, targetRef.current.energy, Math.min(1, dt * 6));

      const motion = reduceMotion ? 0.15 : 1;
      t += dt * (live.speed + live.energy * 1.2) * motion;

      ctx!.clearRect(0, 0, size, size);

      // Ambient halo
      const breathe = 1 + Math.sin(t * 1.4) * 0.04 + live.energy * 0.08;
      const halo = ctx!.createRadialGradient(cx, cy, base * 0.2, cx, cy, base * 2.1 * breathe);
      halo.addColorStop(0, rgba(live.a, 0.35));
      halo.addColorStop(0.45, rgba(live.b, 0.12));
      halo.addColorStop(1, rgba(live.b, 0));
      ctx!.fillStyle = halo;
      ctx!.fillRect(0, 0, size, size);

      if (rings) {
        drawRings();
      }

      ctx!.save();
      ctx!.globalCompositeOperation = "lighter";
      const amp = 0.07 * live.wobble + live.energy * 0.06;
      const layers: Array<{ col: [number, number, number]; seed: number; scale: number; alpha: number }> = [
        { col: live.c, seed: 4.1, scale: 1.08, alpha: 0.35 },
        { col: live.b, seed: 1.7, scale: 1.0, alpha: 0.55 },
        { col: live.a, seed: 0.3, scale: 0.9, alpha: 0.65 },
      ];
      for (const layer of layers) {
        const r = base * layer.scale * breathe;
        const g = ctx!.createRadialGradient(cx, cy - r * 0.2, r * 0.1, cx, cy, r * 1.25);
        g.addColorStop(0, rgba(live.core, layer.alpha));
        g.addColorStop(0.55, rgba(layer.col, layer.alpha * 0.8));
        g.addColorStop(1, rgba(layer.col, 0));
        ctx!.fillStyle = g;
        blobPath(r, layer.seed, amp);
        ctx!.fill();
      }

      // Hot core
      const coreR = base * 0.42 * (1 + Math.sin(t * 2.2) * 0.05 + live.energy * 0.12);
      const core = ctx!.createRadialGradient(cx, cy, 0, cx, cy, coreR);
      core.addColorStop(0, "rgba(255,255,255,0.95)");
      core.addColorStop(0.4, rgba(live.core, 0.7));
      core.addColorStop(1, rgba(live.a, 0));
      ctx!.fillStyle = core;
      ctx!.beginPath();
      ctx!.arc(cx, cy, coreR, 0, Math.PI * 2);
      ctx!.fill();
      ctx!.restore();

      // Glass rim
      ctx!.strokeStyle = rgba(live.core, 0.18);
      ctx!.lineWidth = 1;
      blobPath(base * 1.02 * breathe, 2.2, amp * 0.6);
      ctx!.stroke();

      raf = requestAnimationFrame(frame);
    }

    raf = requestAnimationFrame(frame);
    return () => cancelAnimationFrame(raf);
  }, [size, rings]);

  return (
    <canvas
      ref={canvasRef}
      aria-hidden="true"
      className={className}
      style={{ width: size, height: size }}
    />
  );
}
