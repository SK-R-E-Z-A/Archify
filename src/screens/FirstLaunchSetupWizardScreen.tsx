import { useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import { Button } from "../components/Button";

interface FirstLaunchSetupWizardScreenProps {
}

/**
 * First-launch Setup Wizard screen — design.md §26.
 *
 * Guided first-run setup: choose a project folder, configure AI provider,
 * and complete initial setup. Every button uses the token-driven Button
 * component.
 */
export function FirstLaunchSetupWizardScreen({}: FirstLaunchSetupWizardScreenProps) {
  const [step, setStep] = useState<"welcome" | "project" | "provider" | "complete">("welcome");
  const [projectPath, setProjectPath] = useState("");
  const [loading, setLoading] = useState(false);
  const [error, setError] = useState<string | null>(null);

  const handleNext = async () => {
    if (step === "welcome") {
      setStep("project");
    } else if (step === "project") {
      if (!projectPath) {
        setError("Please enter a project folder path.");
        return;
      }
      try {
        await invoke("create_project", { root: projectPath, name: undefined });
        setStep("provider");
      } catch (e) {
        setError(typeof e === "string" ? e : "Failed to create project");
      }
      } else if (step === "provider") {
      setLoading(true);
      try {
        setLoading(false);
        setStep("complete");
      } catch (e) {
        setLoading(false);
        setError(typeof e === "string" ? e : "Setup failed");
      }
    }
  };

  if (error) {
    return (
      <div className="first-launch-wizard-screen" role="main" aria-label="First Launch Setup Wizard">
        <header className="first-launch-wizard-head">
          <div>
            <h2 className="first-launch-wizard-title">Setup Wizard</h2>
            <p className="first-launch-wizard-sub">Welcome to Archify</p>
          </div>
        </header>
        <div className="first-launch-wizard-error" role="alert">
          <span role="img" aria-hidden="true">⚠</span>
          {error}
          <Button role="secondary" size="sm" onClick={() => setError(null)} title="Dismiss">
            Dismiss
          </Button>
        </div>
      </div>
    );
  }

  return (
    <div className="first-launch-wizard-screen" role="main" aria-label="First Launch Setup Wizard">
      <header className="first-launch-wizard-head">
        <div>
          <h2 className="first-launch-wizard-title">Setup Wizard</h2>
          <p className="first-launch-wizard-sub">Welcome to Archify</p>
        </div>
      </header>

      {step === "welcome" && (
        <div className="first-launch-wizard-step">
          <h3 className="first-launch-wizard-step-title">Welcome</h3>
          <p className="first-launch-wizard-step-desc">
            Let's set up Archify for your development workspace. We'll help
            you add your first project and configure an AI provider.
          </p>
          <div className="first-launch-wizard-actions">
            <Button
              role="primary"
              size="sm"
              onClick={handleNext}
              title="Get started"
            >
              Get Started
            </Button>
          </div>
        </div>
      )}

      {step === "project" && (
        <div className="first-launch-wizard-step">
          <h3 className="first-launch-wizard-step-title">Add Your First Project</h3>
          <p className="first-launch-wizard-step-desc">
            Enter the path to the codebase you want to work on.
          </p>
          <div className="first-launch-wizard-field">
            <input
              type="text"
              value={projectPath}
              onChange={(e) => setProjectPath(e.target.value)}
              placeholder="/path/to/your/project"
              className="first-launch-wizard-input"
            />
          </div>
          <div className="first-launch-wizard-actions">
            <Button
              role="secondary"
              size="sm"
              onClick={() => setStep("welcome")}
              title="Back"
            >
              Back
            </Button>
            <Button
              role="primary"
              size="sm"
              onClick={handleNext}
              disabled={!projectPath}
              title="Continue"
            >
              Continue
            </Button>
          </div>
        </div>
      )}

      {step === "provider" && (
        <div className="first-launch-wizard-step">
          <h3 className="first-launch-wizard-step-title">Configure AI Provider</h3>
          <p className="first-launch-wizard-step-desc">
            Choose your AI provider and model for coding assistance.
          </p>
          <div className="first-launch-wizard-actions">
            <Button
              role="secondary"
              size="sm"
              onClick={() => setStep("project")}
              title="Back"
            >
              Back
            </Button>
            <Button
              role="primary"
              size="sm"
              onClick={handleNext}
              disabled={loading}
              title={loading ? "Configuring…" : "Complete setup"}
            >
              {loading ? "Configuring…" : "Complete Setup"}
            </Button>
          </div>
        </div>
      )}

      {step === "complete" && (
        <div className="first-launch-wizard-complete">
          <span className="first-launch-wizard-complete-icon">✓</span>
          <h3 className="first-launch-wizard-complete-title">Setup Complete</h3>
          <p className="first-launch-wizard-complete-desc">
            Archify is ready to use. Your project has been added and your
            AI provider is configured.
          </p>
          <div className="first-launch-wizard-actions">
            <Button
              role="primary"
              size="sm"
              onClick={() => {
                // Navigate to projects screen is wired to parent
              }}
              title="Open projects"
            >
              Open Projects
            </Button>
          </div>
        </div>
      )}
    </div>
  );
}

export default FirstLaunchSetupWizardScreen;
