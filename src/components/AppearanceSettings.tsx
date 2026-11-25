import React from "react";
import { Button } from "./Button";
import { useDisplay } from "../context/DisplayContext";

type AppearanceTab = "theme" | "density" | "type";

interface AppearanceSettingsProps {
  isOpen: boolean;
  onClose: () => void;
}

type FontSizePreset = "small" | "medium" | "large";

const SIZE_PRESETS: FontSizePreset[] = ["small", "medium", "large"];

/**
 * Appearance settings — design.md §24.2.
 *
 * Theme (Dark Red default / Light), density (Comfortable / Compact),
 * and font-size presets (Small / Medium / Large).
 *
 * Every control uses the token-driven Button component so the settings
 * panel itself demonstrates the responsive button system.
 */
export function AppearanceSettings({
  isOpen,
  onClose,
}: AppearanceSettingsProps) {
  const { theme, setTheme, fontSize, setFontSize, density, setDensity } =
    useDisplay();

  const tabs: { id: AppearanceTab; label: string }[] = [
    { id: "theme", label: "Theme" },
    { id: "density", label: "Density" },
    { id: "type", label: "Type size" },
  ];

  const [activeTab, setActiveTab] = React.useState<AppearanceTab>("theme");

  if (!isOpen) return null;

  return (
    <div
      className="config-modal-backdrop"
      onClick={onClose}
      role="dialog"
      aria-modal="true"
      aria-label="Appearance settings"
    >
      <div
        className="config-modal appearance-modal"
        onClick={(e) => e.stopPropagation()}
        style={{ maxWidth: 560 }}
      >
        <div className="config-modal-header">
          <h2>Appearance</h2>
          <button
            className="icon-button small"
            onClick={onClose}
            title="Close"
            aria-label="Close appearance settings"
          >
            ×
          </button>
        </div>

        <div className="config-modal-body">
          <div className="appearance-tabs">
            {tabs.map((tab) => (
              <button
                key={tab.id}
                type="button"
                className={`appearance-tab ${
                  activeTab === tab.id ? "active" : ""
                }`}
                onClick={() => setActiveTab(tab.id)}
                role="tab"
                aria-selected={activeTab === tab.id}
              >
                {tab.label}
              </button>
            ))}
          </div>

          {activeTab === "theme" && (
            <div className="appearance-section">
              <div className="appearance-section-title">Theme</div>
              <p className="appearance-hint">
                Dark Red is the default. Light theme applies the same design
                system across navigation, dialogs, inputs, tables, graphs,
                terminal, notifications and scrollbars.
              </p>
              <div className="appearance-options">
                <label className="appearance-option">
                  <Button
                    role={theme === "dark" ? "primary" : "secondary"}
                    size="sm"
                    onClick={() => setTheme("dark")}
                    title="Dark Red theme"
                  >
                    Dark Red
                  </Button>
                  <input
                    type="radio"
                    name="theme"
                    value="dark"
                    checked={theme === "dark"}
                    onChange={() => setTheme("dark")}
                    readOnly
                    className="sr-only"
                  />
                </label>
                <label className="appearance-option">
                  <Button
                    role={theme === "light" ? "primary" : "secondary"}
                    size="sm"
                    onClick={() => setTheme("light")}
                    title="Light theme"
                  >
                    Light
                  </Button>
                  <input
                    type="radio"
                    name="theme"
                    value="light"
                    checked={theme === "light"}
                    onChange={() => setTheme("light")}
                    readOnly
                    className="sr-only"
                  />
                </label>
              </div>
            </div>
          )}

          {activeTab === "density" && (
            <div className="appearance-section">
              <div className="appearance-section-title">Density</div>
              <p className="appearance-hint">
                Comfortable gives controls more room; Compact fits more on
                screen. Button padding and height scale accordingly.
              </p>
              <div className="appearance-options">
                <label className="appearance-option">
                  <Button
                    role={density === "comfortable" ? "primary" : "secondary"}
                    size="sm"
                    onClick={() => setDensity("comfortable")}
                    title="Comfortable density"
                  >
                    Comfortable
                  </Button>
                  <input
                    type="radio"
                    name="density"
                    value="comfortable"
                    checked={density === "comfortable"}
                    onChange={() => setDensity("comfortable")}
                    readOnly
                    className="sr-only"
                  />
                </label>
                <label className="appearance-option">
                  <Button
                    role={density === "compact" ? "primary" : "secondary"}
                    size="sm"
                    onClick={() => setDensity("compact")}
                    title="Compact density"
                  >
                    Compact
                  </Button>
                  <input
                    type="radio"
                    name="density"
                    value="compact"
                    checked={density === "compact"}
                    onChange={() => setDensity("compact")}
                    readOnly
                    className="sr-only"
                  />
                </label>
              </div>
            </div>
          )}

          {activeTab === "type" && (
            <div className="appearance-section">
              <div className="appearance-section-title">Type size</div>
              <p className="appearance-hint">
                Small / Medium / Large. Medium is the default. Changing the
                preset consistently updates the interface without clipping.
              </p>
              <div className="appearance-options">
                {SIZE_PRESETS.map((preset) => (
                    <label
                      key={preset}
                      className="appearance-option"
                    >
                      <Button
                        role={
                          fontSize === preset ? "primary" : "secondary"
                        }
                        size="sm"
                        onClick={() => setFontSize(preset)}
                        title={`${preset} type size`}
                      >
                        {preset.charAt(0).toUpperCase() + preset.slice(1)}
                      </Button>
                      <input
                        type="radio"
                        name="font-size"
                        value={preset}
                        checked={fontSize === preset}
                        onChange={() => setFontSize(preset)}
                        readOnly
                        className="sr-only"
                      />
                    </label>
                  ),
                )}
              </div>
            </div>
          )}
        </div>

        <div className="config-modal-footer">
          <Button role="secondary" onClick={onClose}>
            Done
          </Button>
        </div>
      </div>
    </div>
  );
}

export default AppearanceSettings;
