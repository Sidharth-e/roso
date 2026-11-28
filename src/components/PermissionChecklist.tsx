import type { AppConfig, EngineSnapshot } from "../types";
import { CLOUD_OLLAMA_BASE_URL } from "../types";
import { Button, Card, StatusBadge } from "./UiPrimitives";

interface PermissionChecklistProps {
  snapshot: EngineSnapshot | undefined;
  config: AppConfig | undefined;
  onRequestScreen: () => void;
  onRequestAccessibility: () => void;
  onOpenSettings: () => void;
  isRequesting: boolean;
}

export function PermissionChecklist({
  snapshot,
  config,
  onRequestScreen,
  onRequestAccessibility,
  onOpenSettings,
  isRequesting,
}: PermissionChecklistProps) {
  const screenOk = snapshot?.screenPermissionGranted ?? false;
  const axOk = snapshot?.accessibilityPermissionGranted ?? false;
  const isCloud = config
    ? config.ollamaBaseUrl.startsWith(CLOUD_OLLAMA_BASE_URL)
    : true;
  const ollamaReady = config
    ? isCloud
      ? config.hasApiKey && config.cloudConsentAccepted
      : true
    : false;

  if (screenOk && axOk && ollamaReady) {
    return null;
  }

  return (
    <Card className="space-y-2.5 border-warning/40 bg-warning/10">
      <div className="flex items-center justify-between gap-2">
        <span className="text-xs font-bold text-foreground">Setup Checklist</span>
        <StatusBadge tone="warning" label="Action needed" />
      </div>

      <div className="space-y-2">
        <div className="flex items-center justify-between gap-2">
          <div className="min-w-0">
            <p className="text-xs font-semibold text-foreground">
              {screenOk ? "✓ Screen Recording" : "✗ Screen Recording"}
            </p>
            {!screenOk ? (
              <p className="text-xs text-foreground/80">
                Required for screen capture. Relaunch app after granting in System Settings.
              </p>
            ) : null}
          </div>
          {!screenOk ? (
            <Button
              variant="secondary"
              disabled={isRequesting}
              onClick={onRequestScreen}
            >
              Grant
            </Button>
          ) : null}
        </div>

        <div className="flex items-center justify-between gap-2">
          <div className="min-w-0">
            <p className="text-xs font-semibold text-foreground">
              {axOk ? "✓ Accessibility" : "✗ Accessibility"}
            </p>
            {!axOk ? (
              <p className="text-xs text-foreground/80">
                Required to click, type, scroll, and inspect UI elements.
              </p>
            ) : null}
          </div>
          {!axOk ? (
            <Button
              variant="secondary"
              disabled={isRequesting}
              onClick={onRequestAccessibility}
            >
              Grant
            </Button>
          ) : null}
        </div>

        <div className="flex items-center justify-between gap-2">
          <div className="min-w-0">
            <p className="text-xs font-semibold text-foreground">
              {ollamaReady ? "✓ Ollama Ready" : "✗ Ollama Setup"}
            </p>
            {!ollamaReady ? (
              <p className="text-xs text-foreground/80">
                {isCloud
                  ? config?.hasApiKey
                    ? "Accept Ollama Cloud privacy disclosure or switch to local."
                    : "Add an Ollama Cloud API key or switch to local Ollama."
                  : "Configure local Ollama endpoint."}
              </p>
            ) : null}
          </div>
          {!ollamaReady ? (
            <Button variant="secondary" onClick={onOpenSettings}>
              Settings
            </Button>
          ) : null}
        </div>
      </div>
    </Card>
  );
}
