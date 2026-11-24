import { useEffect } from "react";
import { listen } from "@tauri-apps/api/event";
import { getCurrentWindow } from "@tauri-apps/api/window";
import { AnimatePresence, motion } from "framer-motion";
import { PointerCuePayloadSchema } from "../types";
import { useTaskUiStore } from "../store";

export function PointerOverlay() {
  const activeCue = useTaskUiStore((s) => s.activeCue);
  const setActiveCue = useTaskUiStore((s) => s.setActiveCue);

  useEffect(() => {
    const win = getCurrentWindow();
    void win.setIgnoreCursorEvents(true).catch(() => undefined);

    let mounted = true;
    let dismissTimer: ReturnType<typeof setTimeout> | null = null;
    let unlistenFn: (() => void) | null = null;

    listen<unknown>("pointer-cue", (event) => {
      if (!mounted) {
        return;
      }
      void win.setIgnoreCursorEvents(true).catch(() => undefined);
      const parsed = PointerCuePayloadSchema.safeParse(event.payload);
      if (!parsed.success) {
        return;
      }
      if (dismissTimer !== null) {
        clearTimeout(dismissTimer);
      }
      setActiveCue(parsed.data);
      dismissTimer = setTimeout(() => {
        if (mounted) {
          setActiveCue(null);
        }
      }, parsed.data.dismissAfterMs);
    })
      .then((unlisten) => {
        if (mounted) {
          unlistenFn = unlisten;
        } else {
          unlisten();
        }
      })
      .catch(() => undefined);

    return () => {
      mounted = false;
      if (dismissTimer !== null) {
        clearTimeout(dismissTimer);
      }
      if (unlistenFn) {
        unlistenFn();
      }
    };
  }, [setActiveCue]);

  const flipBubbleLeft =
    activeCue !== null &&
    typeof window !== "undefined" &&
    activeCue.targetX > window.innerWidth - 340;

  const flipBubbleUp =
    activeCue !== null &&
    typeof window !== "undefined" &&
    activeCue.targetY > window.innerHeight - 160;

  const bubbleOffsetClass = [
    flipBubbleLeft ? "-translate-x-full -ml-10" : "",
    flipBubbleUp ? "-translate-y-full -mt-2" : "mt-3",
  ]
    .filter(Boolean)
    .join(" ");

  const boxWidth =
    activeCue !== null && activeCue.targetWidth !== null
      ? Math.max(16, activeCue.targetWidth)
      : null;
  const boxHeight =
    activeCue !== null && activeCue.targetHeight !== null
      ? Math.max(16, activeCue.targetHeight)
      : null;

  return (
    <main aria-live="polite" className="overlay-canvas">
      <AnimatePresence mode="wait">
        {activeCue ? (
          <div key={activeCue.id} className="fixed inset-0 pointer-events-none">
            {boxWidth !== null && boxHeight !== null ? (
              <motion.div
                initial={{
                  x: activeCue.targetX - boxWidth * 0.5,
                  y: activeCue.targetY - boxHeight * 0.5,
                  width: boxWidth,
                  height: boxHeight,
                  opacity: 0,
                  scale: 0.92,
                }}
                animate={{
                  x: activeCue.targetX - boxWidth * 0.5,
                  y: activeCue.targetY - boxHeight * 0.5,
                  width: boxWidth,
                  height: boxHeight,
                  opacity: 1,
                  scale: 1,
                }}
                exit={{ opacity: 0, scale: 0.94 }}
                transition={{ duration: 0.2 }}
                className="fixed left-0 top-0 rounded-md border-2 border-primary bg-primary/10 shadow-sm pointer-events-none"
              />
            ) : null}

            <motion.div
              initial={{ x: -120, y: -120, opacity: 0, scale: 0.78 }}
              animate={{
                x: activeCue.targetX,
                y: activeCue.targetY,
                opacity: 1,
                scale: 1,
              }}
              exit={{
                x: activeCue.targetX + 64,
                y: -140,
                opacity: 0,
                scale: 0.82,
              }}
              transition={{
                type: "spring",
                stiffness: 220,
                damping: 22,
                mass: 0.85,
              }}
              className="fixed left-0 top-0 pointer-events-none"
            >
              <div className="relative flex items-start gap-2.5">
                <motion.div
                  animate={{ scale: [1, 1.08, 1], rotate: [0, -5, 0] }}
                  transition={{
                    duration: 1.4,
                    repeat: activeCue.isActing ? Infinity : 1,
                    ease: "easeInOut",
                  }}
                  className="relative"
                >
                  {activeCue.isActing ? (
                    <span className="absolute -left-3 -top-3 h-6 w-6 rounded-full bg-primary/35 animate-ping" />
                  ) : null}
                  <span className="absolute -left-1 -top-1 h-2 w-2 rounded-full bg-primary ring-2 ring-background" />
                  <svg
                    width="28"
                    height="28"
                    viewBox="0 0 28 28"
                    aria-hidden="true"
                    className="drop-shadow-lg"
                  >
                    <path
                      d="M0 0L24 10.5L12.5 14L9 25.5L0 0Z"
                      className="fill-primary stroke-background"
                      strokeWidth="2"
                      strokeLinejoin="round"
                    />
                  </svg>
                </motion.div>

                <motion.div
                  initial={{ opacity: 0, y: 10, scale: 0.94 }}
                  animate={{ opacity: 1, y: 0, scale: 1 }}
                  exit={{ opacity: 0, y: -6, scale: 0.94 }}
                  transition={{
                    type: "spring",
                    stiffness: 280,
                    damping: 24,
                    delay: 0.1,
                  }}
                  className={`speech-bubble ${bubbleOffsetClass}`}
                >
                  <div className="mb-1 flex items-center justify-between gap-2">
                    <span className="text-[10px] font-bold uppercase tracking-wider text-primary">
                      {activeCue.actionBadge} · Step #{activeCue.stepNumber}
                    </span>
                    {activeCue.targetOcrId !== null ? (
                      <span className="rounded bg-primary/15 px-1.5 py-0.5 text-[10px] font-semibold text-primary">
                        #{activeCue.targetOcrId}
                      </span>
                    ) : null}
                  </div>
                  <p className="text-xs font-medium text-foreground">
                    {activeCue.message}
                  </p>
                </motion.div>
              </div>
            </motion.div>
          </div>
        ) : null}
      </AnimatePresence>
    </main>
  );
}
