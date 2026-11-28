import type { TaskPhase } from "../types";
import { Card } from "./UiPrimitives";

interface PlanChecklistProps {
  planSteps: string[];
  activePlanIndex: number;
  phase: TaskPhase;
  showEmptyPlaceholder?: boolean;
}

export function PlanChecklist({
  planSteps,
  activePlanIndex,
  phase,
  showEmptyPlaceholder = false,
}: PlanChecklistProps) {
  if (planSteps.length === 0 && !showEmptyPlaceholder) {
    return null;
  }

  return (
    <Card className="space-y-2">
      <div className="flex items-center justify-between">
        <span className="text-xs font-semibold text-foreground">
          Orchestrated Plan
        </span>
        {planSteps.length > 0 ? (
          <span className="text-xs text-tertiary">
            {planSteps.length} steps
          </span>
        ) : null}
      </div>
      {planSteps.length === 0 ? (
        <p className="text-xs text-tertiary">Waiting for plan generation...</p>
      ) : (
        <ol className="space-y-1">
          {planSteps.map((stepText, idx) => {
            const done = idx < activePlanIndex || phase === "completed";
            const current = idx === activePlanIndex && phase !== "completed";
            return (
              <li
                key={`${idx}-${stepText}`}
                className={`flex items-start gap-2 rounded-md px-2 py-1 text-xs ${
                  current
                    ? "bg-primary/15 font-semibold text-foreground"
                    : done
                      ? "text-tertiary line-through"
                      : "text-foreground/85"
                }`}
              >
                <span className="shrink-0 font-mono text-xs text-primary">
                  {done ? "✓" : `${idx + 1}.`}
                </span>
                <span>{stepText}</span>
              </li>
            );
          })}
        </ol>
      )}
    </Card>
  );
}
