/**
 * Archify — Doctor preflight command
 *
 * Checks environment prerequisites for local development and CI.
 * Outputs PASS, WARNING, or FAIL for each check.
 *
 * Run with: npm run doctor
 */

const { execSync, execFileSync } = require("child_process");
const fs = require("fs");
const path = require("path");
const os = require("os");

const warnings = [];
const failures = [];
const passes = [];

function check(name, condition, detail = "") {
  if (condition) {
    passes.push(`${name}: PASS${detail ? ` — ${detail}` : ""}`);
  } else {
    failures.push(`${name}: FAIL${detail ? ` — ${detail}` : " (no details)"}`);
  }
}

function warn(name, condition, detail = "") {
  if (!condition) {
    warnings.push(`${name}: WARNING${detail ? ` — ${detail}` : ""}`);
  } else {
    passes.push(`${name}: PASS`);
  }
}

// -------------------------------------------------------
// Node.js / npm
// -------------------------------------------------------
const nodeVersion = process.version;
const npmVersion = execSync("npm --version", { encoding: "utf-8" }).trim();
const nodeMajor = parseInt(nodeVersion.replace("v", "").split(".")[0]);

check(
  "Node.js availability",
  !!nodeVersion,
  nodeVersion
);
check(
  "npm availability",
  !!npmVersion,
  npmVersion
);
warn(
  "Node.js version",
  nodeMajor >= 20,
  `Node ${nodeMajor} (recommended: 20+)`
);

// -------------------------------------------------------
// Rust / Cargo
// -------------------------------------------------------
let rustAvailable = false;
let rustStr = "";
try {
  const cargoOutput = execSync("cargo --version", { encoding: "utf-8" });
  rustStr = cargoOutput.trim();
  if (rustStr.length > 0) {
    rustAvailable = true;
  }
} catch (e) {
  // cargo not found
}
warn("Rust/Cargo availability", rustAvailable, rustStr || "not found");
if (rustAvailable && typeof rustStr === "string" && rustStr.length > 3) {
  warn("Rust toolchain version", rustStr.includes("rustc"), rustStr);
}

// -------------------------------------------------------
// Tauri CLI
// -------------------------------------------------------
let tauriAvailable = false;
let tauriStr = "";
try {
  const tauriOutput = execSync("tauri --version", { encoding: "utf-8" });
  tauriStr = tauriOutput.trim();
  if (tauriStr.length > 0) {
    tauriAvailable = true;
  }
} catch (e) {
  // tauri not found
}
warn("Tauri CLI availability", tauriAvailable, tauriStr || "not found");
if (tauriAvailable && typeof tauriStr === "string" && tauriStr.length > 3) {
  warn("Tauri version", tauriStr.includes("2."), tauriStr);
}

// -------------------------------------------------------
// Build tools
// -------------------------------------------------------
let cmakeAvailable = false;
let makeAvailable = false;

try {
  execSync("cmake --version", { encoding: "utf-8" });
  cmakeAvailable = true;
} catch {}

try {
  execSync("make --version", { encoding: "utf-8" });
  makeAvailable = true;
} catch {}

warn("cmake availability", cmakeAvailable, cmakeAvailable ? "found" : "not found");
warn("make availability", makeAvailable, makeAvailable ? "found" : "not found");

// -------------------------------------------------------
// Coding-agent CLI installations
// -------------------------------------------------------
let claudeAvailable = false;
let codexAvailable = false;
let omniflightAvailable = false;

try {
  execSync("claude --version", { encoding: "utf-8", stdio: "pipe" });
  claudeAvailable = true;
} catch {}

try {
  execSync("codex --version", { encoding: "utf-8", stdio: "pipe" });
  codexAvailable = true;
} catch {}

try {
  execSync("omniflight --version", { encoding: "utf-8", stdio: "pipe" });
  omniflightAvailable = true;
} catch {}

warn("Claude Code CLI availability", claudeAvailable, claudeAvailable ? "found" : "not found");
warn("Codex CLI availability", codexAvailable, codexAvailable ? "found" : "not found");
warn("OmniFlight CLI availability", omniflightAvailable, omniflightAvailable ? "found" : "not found");

// -------------------------------------------------------
// Agent executable resolution
// -------------------------------------------------------
let processCliExecutable = "";
try {
  const cmd = process.platform === "win32" ? "where.exe" : "which";
  const result = execFileSync(cmd, ["claude"], {
    encoding: "utf-8",
    timeout: 5000,
    stdio: ["ignore", "pipe", "ignore"],
  });
  if (result) {
    const lines = result.trim().split(/\r?\n/);
    if (lines.length > 0 && lines[0].trim()) {
      processCliExecutable = lines[0].trim();
    }
  }
} catch {}
warn(
  "Process CLI executable resolution",
  processCliExecutable !== "",
  processCliExecutable ? `found at: ${processCliExecutable}` : ""
);

// -------------------------------------------------------
// Provider configuration availability
// -------------------------------------------------------
const configPath = path.join(os.homedir(), ".config", "archify", "provider_config.json");
warn(
  "Provider config directory exists",
  fs.existsSync(configPath),
  fs.existsSync(configPath) ? "found" : "not found"
);

// -------------------------------------------------------
// Relevant configuration and permission problems
// -------------------------------------------------------
// Check if the data directory is writable
const dataDir = path.join(os.homedir(), ".local", "share", "archify");
try {
  fs.mkdirSync(dataDir, { recursive: true });
  check("Data directory writable", true, dataDir);
} catch (e) {
  check("Data directory writable", false, e.message);
}

// Check CLAUDE.md exists
check(
  "CLAUDE.md present",
  fs.existsSync("CLAUDE.md"),
  "found at project root"
);

// -------------------------------------------------------
// Output results
// -------------------------------------------------------
const allOk = failures.length === 0;

console.log("=== Archify Doctor ===\n");

if (passes.length > 0) {
  console.log("PASS:");
  for (const line of passes) {
    console.log(`  ${line}`);
  }
}

if (warnings.length > 0) {
  console.log("\nWARNING:");
  for (const line of warnings) {
    console.log(`  ${line}`);
  }
}

if (failures.length > 0) {
  console.log("\nFAIL:");
  for (const line of failures) {
    console.log(`  ${line}`);
  }
}

console.log(`\n${passes.length} passed, ${warnings.length} warning(s), ${failures.length} failed`);

if (failures.length > 0) {
  console.log(
    "\nRun 'npm run doctor' for details, or see individual prerequisites above."
  );
  process.exit(1);
} else {
  console.log("\nAll critical checks passed.");
  process.exit(0);
}