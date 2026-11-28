import { useEffect } from "react";
import { listen } from "@tauri-apps/api/event";
import { getCurrentWindow } from "@tauri-apps/api/window";
import { AnimatePresence, MotionConfig, motion, useReducedMotion } from "framer-motion";
import { PointerCuePayloadSchema } from "../types";
import { useTaskUiStore } from "../store";

const IS_DEV =
  typeof window !== "undefined" &&
  window.location.hostname === "localhost" &&
  window.location.port === "1420";

export function PointerOverlay() {
  const activeCue = useTaskUiStore((s) => s.activeCue);
  const setActiveCue = useTaskUiStore((s) => s.setActiveCue);
  const prefersReducedMotion = useReducedMotion();

  useEffect(() => {
    const win = getCurrentWindow();
    void win.setIgnoreCursorEvents(true).catch(() => undefined);

    let mounted = true;
    let dismissTimer: ReturnType<typeof setTimeout> | null = null;
    let unlistenCueFn: (() => void) | null = null;
    let unlistenClearFn: (() => void) | null = null;

    listen<unknown>("pointer-cue", (event) => {
      if (!mounted) {
        return;
      }
      void win.setIgnoreCursorEvents(true).catch(() => undefined);
      const parsed = PointerCuePayloadSchema.safeParse(event.payload);
      if (!parsed.success) {
        if (IS_DEV) {
          console.warn("Invalid pointer-cue payload", parsed.error);
        }
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
          unlistenCueFn = unlisten;
        } else {
          unlisten();
        }
      })
      .catch((err: unknown) => {
        if (IS_DEV) {
          console.warn("Failed to listen for pointer-cue", err);
        }
      });

    listen<unknown>("clear-pointer-cue", () => {
      if (!mounted) {
        return;
      }
      if (dismissTimer !== null) {
        clearTimeout(dismissTimer);
        dismissTimer = null;
      }
      setActiveCue(null);
    })
      .then((unlisten) => {
        if (mounted) {
          unlistenClearFn = unlisten;
        } else {
          unlisten();
        }
      })
      .catch((err: unknown) => {
        if (IS_DEV) {
          console.warn("Failed to listen for clear-pointer-cue", err);
        }
      });

    return () => {
      mounted = false;
      if (dismissTimer !== null) {
        clearTimeout(dismissTimer);
      }
      if (unlistenCueFn) {
        unlistenCueFn();
      }
      if (unlistenClearFn) {
        unlistenClearFn();
      }
    };
  }, [setActiveCue]);

  const boxWidth =
    activeCue !== null && activeCue.showPointer && activeCue.targetWidth !== null
      ? Math.max(16, activeCue.targetWidth)
      : null;
  const boxHeight =
    activeCue !== null && activeCue.showPointer && activeCue.targetHeight !== null
      ? Math.max(16, activeCue.targetHeight)
      : null;

  const pointerAnchorX =
    activeCue !== null
      ? boxWidth !== null
        ? activeCue.targetX + boxWidth * 0.5 + 4
        : activeCue.targetX + 6
      : 0;
  const pointerAnchorY =
    activeCue !== null
      ? boxHeight !== null
        ? activeCue.targetY + boxHeight * 0.5 + 4
        : activeCue.targetY + 6
      : 0;

  const flipBubbleLeft =
    activeCue !== null &&
    activeCue.showPointer &&
    typeof window !== "undefined" &&
    pointerAnchorX > window.innerWidth - 340;

  const flipBubbleUp =
    activeCue !== null &&
    activeCue.showPointer &&
    typeof window !== "undefined" &&
    pointerAnchorY > window.innerHeight - 160;

  const bubbleOffsetClass = activeCue?.showPointer
    ? [
        flipBubbleLeft ? "-translate-x-full -ml-10" : "",
        flipBubbleUp ? "-translate-y-full -mt-2" : "mt-3",
      ]
        .filter(Boolean)
        .join(" ")
    : "-translate-x-1/2 mt-2";

  return (
    <MotionConfig reducedMotion="user">
      <main aria-live="polite" className="overlay-canvas">
        <AnimatePresence>
          {activeCue ? (
            <div key="persistent-overlay-cue" className="fixed inset-0 pointer-events-none">
              {boxWidth !== null && boxHeight !== null ? (
                <motion.div
                  initial={{
                    x: activeCue.targetX - boxWidth * 0.5,
                    y: activeCue.targetY - boxHeight * 0.5,
                    width: boxWidth,
                    height: boxHeight,
                    opacity: 0,
                    scale: 0.94,
                  }}
                  animate={{
                    x: activeCue.targetX - boxWidth * 0.5,
                    y: activeCue.targetY - boxHeight * 0.5,
                    width: boxWidth,
                    height: boxHeight,
                    opacity: 1,
                    scale: 1,
                  }}
                  exit={{ opacity: 0, scale: 0.95 }}
                  transition={{ duration: 0.16 }}
                  className="fixed left-0 top-0 rounded-md border-2 border-primary bg-primary/10 shadow-sm pointer-events-none"
                />
              ) : null}

              <motion.div
                initial={{
                  x: pointerAnchorX,
                  y: pointerAnchorY - 18,
                  opacity: 0,
                  scale: 0.9,
                }}
                animate={{
                  x: pointerAnchorX,
                  y: pointerAnchorY,
                  opacity: 1,
                  scale: 1,
                }}
                exit={{
                  opacity: 0,
                  scale: 0.9,
                }}
                transition={{
                  type: "spring",
                  stiffness: 380,
                  damping: 30,
                  mass: 0.55,
                }}
                className="fixed left-0 top-0 pointer-events-none"
              >
                <div className="relative flex items-start gap-2.5">
                  {activeCue.showPointer ? (
                    <motion.div
                      animate={
                        prefersReducedMotion
                          ? undefined
                          : { scale: [1, 1.08, 1], rotate: [0, -5, 0] }
                      }
                      transition={{
                        duration: 1.4,
                        repeat: activeCue.isActing ? Infinity : 1,
                        ease: "easeInOut",
                      }}
                      className="relative"
                    >
                      {activeCue.isActing && !prefersReducedMotion ? (
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
                  ) : null}

                  <motion.div
                    initial={{ opacity: 0, y: 8, scale: 0.95 }}
                    animate={{ opacity: 1, y: 0, scale: 1 }}
                    exit={{ opacity: 0, y: -4, scale: 0.95 }}
                    transition={{
                      type: "spring",
                      stiffness: 420,
                      damping: 30,
                    }}
                    className={`speech-bubble ${bubbleOffsetClass}`}
                  >
                    <div className="mb-1 flex items-center justify-between gap-2">
                      <span className="text-[10px] font-bold uppercase tracking-wider text-primary">
                        {activeCue.actionBadge} · Step #{activeCue.stepNumber}
                      </span>
                      {activeCue.showPointer && activeCue.targetOcrId !== null ? (
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
    </MotionConfig>
  );
}
