import { useEffect, useState, type FormEvent } from "react";
import {
  CLOUD_OLLAMA_BASE_URL,
  DEFAULT_MODEL_ROUTING,
  LOCAL_OLLAMA_BASE_URL,
  type AppConfig,
  type ModelRoutingConfig,
  type UpdateAppConfigPayload,
} from "../types";
import { Button, Card, Input, StatusBadge, Switch } from "./UiPrimitives";

interface SettingsFormProps {
  config: AppConfig;
  isSaving: boolean;
  saveError: string | null;
  saveSuccess: boolean;
  isTesting: boolean;
  availableModels: string[];
  connectionStatus: { ok: boolean; message: string } | null;
  onSave: (payload: UpdateAppConfigPayload) => void;
  onTestConnection: (baseUrl: string) => void;
}

export function SettingsForm({
  config,
  isSaving,
  saveError,
  saveSuccess,
  isTesting,
  availableModels,
  connectionStatus,
  onSave,
  onTestConnection,
}: SettingsFormProps) {
  const [baseUrl, setBaseUrl] = useState(config.ollamaBaseUrl);
  const [apiKeyInput, setApiKeyInput] = useState("");
  const [clearApiKey, setClearApiKey] = useState(false);
  const [cloudConsentAccepted, setCloudConsentAccepted] = useState(
    config.cloudConsentAccepted
  );
  const [models, setModels] = useState<ModelRoutingConfig>(config.models);

  useEffect(() => {
    setBaseUrl(config.ollamaBaseUrl);
    setCloudConsentAccepted(config.cloudConsentAccepted);
    setModels(config.models);
    setApiKeyInput("");
    setClearApiKey(false);
  }, [config]);

  const isCloud = baseUrl.trim().startsWith(CLOUD_OLLAMA_BASE_URL);

  function updateModelField<K extends keyof ModelRoutingConfig>(
    key: K,
    value: ModelRoutingConfig[K]
  ) {
    setModels((prev) => ({
      ...prev,
      [key]: value,
    }));
  }

  function handleSubmit(event: FormEvent) {
    event.preventDefault();
    onSave({
      ollamaBaseUrl: baseUrl.trim() || CLOUD_OLLAMA_BASE_URL,
      ollamaApiKey: apiKeyInput.trim(),
      clearApiKey,
      cloudConsentAccepted,
      models,
    });
  }

  return (
    <form onSubmit={handleSubmit} className="space-y-3">
      <Card className="space-y-2.5">
        <div className="flex items-center justify-between gap-2">
          <span className="text-xs font-semibold text-foreground">
            Ollama Endpoint Mode
          </span>
          <StatusBadge
            tone={isCloud ? "info" : "success"}
            label={isCloud ? "Cloud" : "Local"}
          />
        </div>

        <div className="grid grid-cols-2 gap-1.5 rounded-lg bg-background/80 p-1">
          <button
            type="button"
            onClick={() => setBaseUrl(CLOUD_OLLAMA_BASE_URL)}
            className={`rounded-md py-1.5 text-xs font-semibold transition focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-primary/40 ${
              isCloud
                ? "bg-primary text-background"
                : "text-tertiary hover:text-foreground"
            }`}
          >
            Ollama Cloud
          </button>
          <button
            type="button"
            onClick={() => setBaseUrl(LOCAL_OLLAMA_BASE_URL)}
            className={`rounded-md py-1.5 text-xs font-semibold transition focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-primary/40 ${
              !isCloud
                ? "bg-primary text-background"
                : "text-tertiary hover:text-foreground"
            }`}
          >
            Local Ollama
          </button>
        </div>

        <Input
          id="ollama-base-url"
          label="Endpoint URL"
          type="text"
          value={baseUrl}
          onChange={(e) => setBaseUrl(e.target.value)}
          placeholder={LOCAL_OLLAMA_BASE_URL}
        />

        {isCloud ? (
          <div className="space-y-2 rounded-lg border border-warning/35 bg-warning/10 p-2.5">
            <Switch
              id="toggle-cloud-consent"
              checked={cloudConsentAccepted}
              onChange={setCloudConsentAccepted}
              label="Allow Cloud Screen Inspection"
              description="In-memory screenshots, window titles, and visible target text are sent to ollama.com during active runs."
            />
          </div>
        ) : null}

        <div className="space-y-1.5">
          <Input
            id="ollama-api-key"
            label={
              config.hasApiKey && !clearApiKey
                ? `Ollama API Key (Saved: ${config.apiKeyPreview})`
                : "Ollama API Key"
            }
            type="password"
            placeholder={
              config.hasApiKey && !clearApiKey
                ? "Leave blank to keep existing key in Keychain"
                : "Paste key to store in macOS Keychain"
            }
            value={apiKeyInput}
            onChange={(e) => {
              setClearApiKey(false);
              setApiKeyInput(e.target.value);
            }}
          />
          {config.hasApiKey ? (
            <div className="flex items-center justify-between">
              <span className="text-xs text-tertiary">
                {clearApiKey ? "Key will be cleared on save" : "Stored privately"}
              </span>
              <button
                type="button"
                onClick={() => {
                  setClearApiKey((prev) => !prev);
                  setApiKeyInput("");
                }}
                className="text-xs font-semibold text-error hover:underline focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-primary/40 rounded"
              >
                {clearApiKey ? "Undo Clear" : "Clear Saved Key"}
              </button>
            </div>
          ) : null}
        </div>

        <div className="flex items-center justify-between gap-2 pt-1">
          <Button
            variant="secondary"
            disabled={isTesting}
            onClick={() => onTestConnection(baseUrl)}
          >
            {isTesting ? "Testing..." : "Test Connection"}
          </Button>
          {connectionStatus ? (
            <span
              className={`text-xs font-medium ${
                connectionStatus.ok ? "text-success" : "text-error"
              }`}
            >
              {connectionStatus.message}
            </span>
          ) : null}
        </div>
      </Card>

      <datalist id="ollama-available-models">
        {availableModels.map((name) => (
          <option key={name} value={name} />
        ))}
      </datalist>

      <Card className="space-y-2.5">
        <div className="flex items-center justify-between gap-2">
          <span className="text-xs font-semibold text-foreground">
            Multi-Model Routing
          </span>
          <button
            type="button"
            onClick={() => setModels({ ...DEFAULT_MODEL_ROUTING })}
            className="text-xs font-semibold text-primary hover:underline focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-primary/40 rounded px-1"
          >
            Reset Defaults
          </button>
        </div>
        <Input
          id="model-judge"
          label="Judge & Classifier Model"
          type="text"
          list="ollama-available-models"
          value={models.judgeModel}
          onChange={(e) => updateModelField("judgeModel", e.target.value)}
        />
        <Input
          id="model-planner"
          label="Planner Model"
          type="text"
          list="ollama-available-models"
          value={models.plannerModel}
          onChange={(e) => updateModelField("plannerModel", e.target.value)}
        />
        <Input
          id="model-vision-actor"
          label="Vision / General UI Actor Model"
          type="text"
          list="ollama-available-models"
          value={models.visionActorModel}
          onChange={(e) =>
            updateModelField("visionActorModel", e.target.value)
          }
        />
        <Input
          id="model-coding-actor"
          label="Coding / Terminal Actor Model"
          type="text"
          list="ollama-available-models"
          value={models.codingActorModel}
          onChange={(e) =>
            updateModelField("codingActorModel", e.target.value)
          }
        />
        <Input
          id="model-writing-actor"
          label="Writing / Notes Actor Model"
          type="text"
          list="ollama-available-models"
          value={models.writingActorModel}
          onChange={(e) =>
            updateModelField("writingActorModel", e.target.value)
          }
        />
        <Input
          id="model-recovery"
          label="Recovery Specialist Model"
          type="text"
          list="ollama-available-models"
          value={models.recoveryModel}
          onChange={(e) => updateModelField("recoveryModel", e.target.value)}
        />
      </Card>

      <Card className="space-y-3">
        <span className="text-xs font-semibold text-foreground">
          Orchestration Options
        </span>
        <Switch
          id="toggle-judge-verify"
          checked={models.enableJudgeVerification}
          onChange={(checked) =>
            updateModelField("enableJudgeVerification", checked)
          }
          label="Judge Step Verification"
          description="Judge verifies stalled steps and completion before stopping."
        />
      </Card>

      <Card className="space-y-2">
        <Button type="submit" className="w-full" disabled={isSaving}>
          {isSaving ? "Saving..." : "Save Configuration"}
        </Button>
        {saveError ? <p className="text-xs text-error">{saveError}</p> : null}
        {saveSuccess ? (
          <p className="text-xs text-success">
            Configuration saved securely.
          </p>
        ) : null}
      </Card>
    </form>
  );
}
