import type { TaskPhase } from "../types";
import { Button, phaseBadgeProps } from "./UiPrimitives";

interface RunControlsProps {
  phase: TaskPhase;
  isBusy: boolean;
  onPause: () => void;
  onResume: () => void;
  onStop: () => void;
}

export function RunControls({
  phase,
  isBusy,
  onPause,
  onResume,
  onStop,
}: RunControlsProps) {
  const badge = phaseBadgeProps(phase);

  return (
    <div className="grid grid-cols-2 gap-2">
      {phase === "running" ? (
        <Button variant="secondary" disabled={isBusy} onClick={onPause}>
          Pause
        </Button>
      ) : phase === "paused" ? (
        <Button variant="primary" disabled={isBusy} onClick={onResume}>
          Resume
        </Button>
      ) : (
        <Button variant="secondary" disabled>
          {badge.label}...
        </Button>
      )}
      <Button variant="danger" disabled={isBusy} onClick={onStop}>
        Stop (⌃⌥S)
      </Button>
    </div>
  );
}
