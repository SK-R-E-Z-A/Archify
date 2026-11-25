# ARCHIFY — COMPLETE UI/UX DESIGN SPECIFICATION

**Document type:** Master UI/UX Design Requirements  
**Product:** Archify Desktop Application  
**Design status:** Six-round design interview completed; decisions consolidated  
**Platform:** Windows desktop, built with Tauri v2, React and TypeScript  
**Primary design objective:** Create a premium, sophisticated, modern AI development command center with powerful project management, multi-agent orchestration, execution monitoring and automation capabilities.

---

# 1. OVERALL PRODUCT AND DESIGN VISION

## 1.1 Product identity

Archify should feel like a premium, purpose-built AI development environment rather than a generic dashboard, ordinary chat application, basic coding-agent wrapper or collection of unrelated tools.

The application should bring together:

- AI agent orchestration.
- Project management.
- AI provider and model management.
- Task planning and execution.
- Live execution visualization.
- Conversation management.
- Automation management.
- Skills and plugin management.
- Terminal access.
- Repository inspection.
- Agent monitoring.
- Project architecture visualization.
- Reports, activity history and recovery.
- Global application configuration.

The experience should feel like a unified command center for managing multiple AI-powered development projects.

## 1.2 Core design philosophy

The design must balance four principles:

1. **Premium simplicity:** The interface should look clean, refined and deliberate without becoming visually empty.
2. **Powerful information density:** Advanced functionality should be accessible without overwhelming the user.
3. **Clear hierarchy:** Important information, execution states, navigation and actions must be immediately understandable.
4. **Operational visibility:** Users should always understand what their agents and projects are doing.

Archify must not look like a basic CRUD dashboard with a red theme applied to it.

The objective is a complete, thoughtfully designed application with its own visual identity, interaction language and consistent component system.

## 1.3 Design personality

The application should communicate:

- Professionalism.
- Precision.
- Intelligence.
- Control.
- Technical sophistication.
- Reliability.
- Exclusivity.
- Confidence.
- Modernity.

Avoid designs that feel:

- Generic.
- Childish.
- Overly colorful.
- Excessively decorative.
- Cluttered.
- Inconsistent.
- Like a template dashboard.
- Like a default component-library installation.
- Like a simple web application placed inside a desktop window.

---

# 2. BRAND IDENTITY AND COLOR SYSTEM

## 2.1 Primary color direction

Archify must use a predominantly black and crimson-red visual identity, supported by champagne gold and readable neutral colors.

Approximate visual distribution:

- Black and near-black: 60%.
- Crimson and deep red: 25%.
- Champagne gold: 5%.
- Off-white and gray: 10%.

These are approximate overall visual proportions, not strict quotas for every screen.

## 2.2 Color roles

### Black and near-black

Black should establish the application's visual foundation.

Use it for:

- Main application background.
- Primary workspace surfaces.
- Large dashboard surfaces.
- Navigation background.
- Major application regions.
- Deep visual separation.

Use multiple carefully selected near-black shades rather than one flat black color everywhere.

### Crimson and deep red

Red is Archify's signature color.

Use it for:

- Primary actions.
- Active navigation indicators.
- Selected states.
- Important execution indicators.
- Key interactive elements.
- Progress indicators where appropriate.
- Important highlights.
- Brand identity elements.

Red should be distinctive and intentional. Do not paint every card, border, icon and button red.

### Champagne gold

Gold is a refined accent.

Use it sparingly for:

- Premium highlights.
- Important milestones.
- Special status indicators.
- Selected decorative details.
- Subtle brand signatures.

Gold must not compete with red as the primary accent.

### Off-white and gray

Use neutral colors for:

- Main text.
- Secondary text.
- Supporting labels.
- Borders.
- Dividers.
- Muted interface elements.
- Disabled states.
- Metadata.

Maintain strong readability throughout the interface.

## 2.3 Theme support

Archify must include:

- Dark Red theme as the default.
- Light theme.

Theme changes must affect the entire application consistently.

This includes:

- Navigation.
- Dialogs.
- Menus.
- Inputs.
- Tables.
- Graphs.
- Charts.
- Terminal surfaces.
- Notifications.
- Scrollbars.
- Context menus.
- Loading indicators.
- Empty states.

Do not create a dark theme that is polished while leaving the light theme incomplete.

---

# 3. TYPOGRAPHY AND VISUAL HIERARCHY

## 3.1 Typography

Use a modern, readable interface typography system.

Typography should establish a clear distinction between:

- Screen titles.
- Section headings.
- Card headings.
- Body text.
- Secondary descriptions.
- Labels.
- Metadata.
- Technical information.
- Terminal output.
- Code-related content.

Avoid oversized headings that consume unnecessary workspace space.

## 3.2 Font-size presets

Provide three user-selectable presets:

- Small.
- Medium.
- Large.

Medium should be the default.

Changing the preset should consistently update the interface without breaking layout or causing clipping.

## 3.3 Typography behavior

- Use consistent font weights.
- Avoid excessive bold text.
- Maintain readable line lengths.
- Use appropriate spacing between headings and content.
- Ensure metadata is distinguishable without becoming illegible.
- Use monospace typography for terminal output and technical identifiers where appropriate.
- Preserve readability in dense tables and monitoring views.

---

# 4. APPLICATION SHELL AND GLOBAL LAYOUT

## 4.1 Main application structure

The application should have a consistent desktop shell containing:

1. Global sidebar.
2. Main content workspace.
3. Contextual details area where applicable.
4. Persistent status and notification surfaces.
5. Optional repository panel.
6. Optional terminal panel.

The main application layout must remain consistent when navigating between destinations.

## 4.2 Global sidebar

The sidebar is the primary navigation system.

It must support three states:

- Expanded.
- Compact.
- Hidden.

The expanded state displays navigation labels and relevant information.

The compact state displays recognizable navigation icons.

The hidden state maximizes available workspace.

The sidebar should have a clear visual distinction between global navigation, project navigation and bottom-anchored settings.

## 4.3 Global sidebar navigation order

Use this exact order:

1. Overview.
2. Skills.
3. Plugins.
4. Automation.
5. Projects.

Settings must be anchored at the bottom of the sidebar.

Global destinations such as Activity, Reports, Agent Console, Recovery Center, Notifications, Archive and Trash must remain accessible through the global command center/navigation structure.

Do not randomly rearrange the approved navigation hierarchy.

## 4.4 Project navigation

The Projects area should support:

- Multiple projects.
- Project names.
- Project status indicators.
- Pinned projects.
- Project tags.
- Project navigation expansion.
- Project context switching.

A project must support two distinct interactions:

- Clicking the project opens it.
- Clicking its separate chevron expands or collapses its project navigation.

Do not make a project require an accidental double-click to open.

## 4.5 Project context menu

Each project must have a three-dot menu with:

1. Open.
2. New Task.
3. Rename.
4. Pin.
5. Duplicate.
6. Project Settings.
7. Archive.
8. Remove.

Destructive actions must require appropriate confirmation.

Removing a project must distinguish between keeping its files and deleting them.

## 4.6 Main workspace

The central workspace should receive the majority of available screen space.

It must support:

- Consistent content margins.
- Responsive layouts.
- Clear screen headings.
- Appropriate section spacing.
- Context-sensitive panels.
- Smooth navigation transitions.
- Proper scrolling behavior.

Avoid unnecessarily large empty spaces and excessively compressed content.

## 4.7 Panel positioning

The main application panels must have defined, predictable positions.

Users can resize and collapse supported panels, but they must not freely drag and rearrange the entire application layout.

Dashboard widgets are a separate exception and are independently customizable.

---

# 5. OVERVIEW DASHBOARD

## 5.1 Purpose

Overview is the global landing screen and the user's command center.

It should communicate the overall state of Archify immediately.

It must not be a static collection of decorative cards.

## 5.2 Dashboard customization

Users must be able to:

- Add widgets.
- Remove widgets.
- Drag widgets.
- Rearrange widgets.
- Resize widgets.
- Configure widget content.
- Customize dashboard sections.

Widget arrangements should persist between application sessions.

## 5.3 Potential dashboard widgets

Provide an extensible widget system capable of supporting:

- Total projects.
- Active projects.
- Currently executing tasks.
- Agent activity.
- Agent availability.
- Recent tasks.
- Recent conversations.
- Recent activity.
- Automation status.
- Recent automation runs.
- Recent reports.
- Notifications.
- Failed tasks.
- Recovery-required tasks.
- Project summaries.

The widget system must remain modular so additional widgets can be introduced later.

## 5.4 Dashboard behavior

- Widgets should have consistent spacing.
- Widgets should support useful loading states.
- Widgets should have meaningful empty states.
- Widget configuration should not affect the global application layout.
- Dashboard customization should be persistent.
- Users should be able to restore a sensible default arrangement.

---

# 6. GLOBAL SEARCH AND COMMAND PALETTE

These are two distinct capabilities.

## 6.1 Global search

Global search must search across relevant Archify content, including:

- Projects.
- Tasks.
- Conversations.
- Reports.
- Automations.
- Skills.
- Plugins.
- Agents.
- Other searchable application resources.

Provide appropriate result categorization and filtering.

Search results should open the relevant destination or selected item.

## 6.2 Command palette

The command palette is an action and navigation interface.

It should support:

- Navigating to screens.
- Opening projects.
- Switching workspaces.
- Creating projects.
- Creating tasks.
- Starting conversations.
- Opening settings.
- Launching relevant management views.
- Performing supported application actions.

The command palette should be keyboard-friendly.

Search and command execution must not be confused with one another.

## 6.3 Keyboard shortcuts

Implement a consistent shortcut system for common actions.

At minimum, consider:

- Open command palette.
- Open global search.
- Navigate to Overview.
- Open project switcher.
- Create project.
- Create task.
- Toggle sidebar.
- Toggle terminal.
- Open notifications.
- Navigate between relevant workspace tabs.

Provide a shortcut reference overlay and avoid conflicting shortcuts.

---

# 7. PROJECT MANAGEMENT

## 7.1 Multiple projects

Archify must support multiple independent projects.

Each project should maintain its own:

- Project settings.
- Tasks.
- Conversations.
- Agent activity.
- Execution state.
- Terminal tabs.
- Project Map.
- Reports.
- Activity history.
- Repository context.
- Relevant automation configuration.

## 7.2 Project organization

Support:

- Project creation.
- Project renaming.
- Project duplication.
- Project pinning.
- Project tags.
- Project archiving.
- Project removal.
- Project restoration.

## 7.3 Project concurrency

Multiple projects must be able to execute simultaneously.

However:

**Maximum one executing task per individual project.**

Different projects can execute tasks concurrently without requiring the entire application to serialize all work.

The interface must clearly communicate:

- Which projects are active.
- Which tasks are executing.
- Which agents are working.
- Which projects are waiting.
- Which projects have failed tasks.
- Which projects require user attention.

## 7.4 Archive and Trash

Archive and Trash are separate global destinations.

Archive:

- Contains archived projects.
- Allows restoration.
- Keeps archived projects separate from active project navigation.

Trash:

- Contains removed projects.
- Allows restoration.
- Supports permanent deletion.
- Supports an explicit empty-trash action.

Permanent deletion must be protected by confirmation.

---

# 8. PROJECT PLAYGROUND

## 8.1 Purpose

The Playground is the main working environment of each project.

When opening a project, the Playground should be selected by default.

The default selected tab must be Graph.

The Playground contains two separate tabs:

1. Graph.
2. Chat.

## 8.2 Graph tab

The Graph is the live execution visualization.

It should display a connected execution tree containing:

- Parent tasks.
- Subtasks.
- Dependencies.
- Agents.
- Execution steps.
- Affected files.
- Task states.
- Execution progress.

## 8.3 Graph interactions

Users must be able to:

- Select nodes.
- Expand nodes.
- Collapse nodes.
- Inspect node details.
- Follow live progress.
- Inspect dependencies.
- Understand task relationships.
- Switch between supported graph layouts.

Graph interactions must be predictable and visually clear.

## 8.4 Graph visual language

Different node types must be visually distinguishable.

Use consistent shapes, labels, icons and state indicators.

Execution states should be understandable without relying on color alone.

The graph must communicate:

- Pending.
- Queued.
- Running.
- Completed.
- Failed.
- Blocked.
- Cancelled.
- Awaiting user attention.

Avoid visual clutter and excessive decorative animation.

## 8.5 Chat tab

The Chat tab provides the project's conversation and agent interaction interface.

Users must be able to:

- Send prompts.
- Review responses.
- View conversation history.
- Select an agent.
- Select a model.
- Select a provider.
- Choose Plan mode.
- Choose Execute mode.
- Attach files.
- Include context files.
- View relevant execution information.

The user explicitly chooses whether the interaction is in Plan or Execute mode.

## 8.6 Chat composer menu

The composer menu must provide access to:

- Agent selection.
- Model selection.
- Provider selection.
- Plan/Execute mode.
- Context file selection.
- File attachments.

Keep the composer usable and uncluttered.

Advanced controls should be accessible without occupying unnecessary permanent space.

---

# 9. PROJECT TASK MANAGEMENT

## 9.1 Project Tasks screen

Each project has its own Tasks destination.

It should provide a structured view of:

- Task name.
- Description.
- Status.
- Priority where supported.
- Assigned agent.
- Creation time.
- Last updated time.
- Execution progress.
- Related conversation.
- Related automation where applicable.

Provide useful filtering and search.

## 9.2 Task lifecycle

The task interface should communicate the task lifecycle clearly.

Possible states include:

- Draft.
- Planned.
- Queued.
- Running.
- Completed.
- Failed.
- Blocked.
- Cancelled.
- Awaiting review.
- Awaiting user attention.

Do not display fabricated progress or imply completion before verification.

## 9.3 Task Detail screen

Each task opens a dedicated Task Detail destination.

Include:

1. Task overview.
2. Execution plan.
3. Subtasks.
4. Execution timeline.
5. Agent activity.
6. Logs.
7. Files changed.
8. Verification results.
9. Review results.
10. Recovery actions.

The task details should give users enough information to understand what happened and what needs to happen next.

---

# 10. CONVERSATION MANAGEMENT

## 10.1 Project conversations

Each project supports multiple independent conversations.

Conversations must be organized into folders or categories.

## 10.2 Conversation management

Support:

- Creating conversations.
- Renaming conversations.
- Searching conversations.
- Organizing conversations.
- Moving conversations between folders or categories.
- Accessing conversation history.
- Attaching files.
- Selecting agent/model/provider.
- Choosing Plan/Execute mode.

## 10.3 Conversation persistence

Conversation data must be saved reliably.

The interface should communicate saving and loading states accurately.

Avoid losing conversation state during navigation or application restarts.

---

# 11. AGENT MANAGEMENT AND MONITORING

## 11.1 Agent architecture

Archify must maintain distinct agent roles.

Core roles:

- Prompt Agent: planning, reviewing, evaluating execution and generating corrective prompts.
- Coding Agent: executing coding tasks through supported installed CLI agents.
- Additional agents: extensible for future functionality.

The visual interface must make agent responsibilities understandable.

## 11.2 Global Agent Console

Provide a dedicated global Agent Console.

It should monitor agents across all projects.

Include:

- Agent identity.
- Connected provider.
- Selected model.
- Current project.
- Current task.
- Current status.
- Current activity.
- Recent output.
- Errors.
- Attention requirements.

Provide project filters.

## 11.3 Project Agent Console

Every project also has its own Agent Console.

It should focus exclusively on the agents operating within that project.

Users should be able to inspect relevant agent activity without navigating to the global console.

## 11.4 Individual agent configuration

Each agent has its own configuration destination.

Include:

- Agent identity.
- Provider assignment.
- Model assignment.
- Instructions.
- Permissions.
- Behavior controls.
- Configuration status.

## 11.5 Agent monitoring states

Clearly distinguish:

- Idle.
- Initializing.
- Planning.
- Executing.
- Reviewing.
- Waiting.
- Paused.
- Completed.
- Failed.
- Disconnected.
- Awaiting approval.

Do not present an agent as active when it is disconnected or not executing.

---

# 12. EXECUTION ORCHESTRATION AND RECOVERY

## 12.1 Core execution workflow

Archify should support a structured execution lifecycle:

1. Receive task.
2. Plan task.
3. Generate execution instructions.
4. Execute through the Coding Agent.
5. Collect execution report.
6. Review results.
7. Verify outcomes.
8. Generate corrective instructions if needed.
9. Retry or continue execution.
10. Complete and report.

The interface must represent the actual state of this lifecycle.

## 12.2 Execution visibility

Users should be able to understand:

- What is happening.
- Which agent is responsible.
- What the agent is working on.
- What has completed.
- What is waiting.
- What failed.
- What requires intervention.

## 12.3 Global Recovery Center

Provide a dedicated Recovery Center.

It should centralize:

- Failed tasks.
- Blocked tasks.
- Tasks requiring user intervention.
- Execution errors.
- Recovery attempts.
- Relevant logs.
- Recovery status.

Users should be able to filter and inspect recovery items across projects.

## 12.4 Task-level recovery

Task Detail must also provide contextual recovery actions.

Potential actions:

- Retry.
- Resume.
- Stop.
- Inspect logs.
- Regenerate corrective prompt.
- Mark for manual intervention.
- Review the failure.
- Return to planning where supported.

## 12.5 Recovery accuracy

The application must never claim that a failed task has been recovered before the recovery operation actually succeeds.

Display meaningful status and error information.

---

# 13. PROJECT MAP

## 13.1 Purpose

Project Map is a separate project destination.

It is not the same as the Playground's live execution graph.

The Project Map represents broader project structure.

## 13.2 Information represented

Include relevant relationships between:

- Architecture.
- Tasks.
- Dependencies.
- Connected resources.
- Project components.
- Other supported structural elements.

## 13.3 Interaction

Allow users to explore relevant relationships and inspect selected elements.

The Project Map should be useful for understanding the project, not merely decorative.

Keep the visual language consistent with the execution graph while giving both visualizations clearly different purposes.

---

# 14. REPOSITORY PANEL

## 14.1 Purpose

Provide a read-only repository tree within the project workspace.

This is not a built-in code editor or complete file-management system.

## 14.2 Required functionality

- Display folders.
- Display files.
- Expand and collapse folders.
- Select items.
- Open files through supported external or system actions.
- Reveal items in the operating system.
- Resize the panel.

## 14.3 Restrictions

Do not add:

- Built-in file editing.
- File deletion.
- File renaming.
- Drag-and-drop file rearrangement.
- Full Git interface.

The repository panel is for inspection and navigation, not replacing a dedicated development environment.

---

# 15. TERMINAL PANEL

## 15.1 Default behavior

The terminal is collapsed by default.

Users can expand it when necessary.

## 15.2 Project terminal model

Each project has one terminal environment containing multiple terminal tabs.

The terminal must not behave like one shared terminal that unexpectedly mixes different projects.

## 15.3 Required behavior

- Expand and collapse.
- Create terminal tabs.
- Switch between tabs.
- Close tabs.
- Display terminal output.
- Maintain appropriate project context.
- Resize the terminal panel.

## 15.4 Visual design

Use a clean, readable terminal presentation.

Terminal output should use appropriate monospace typography.

Maintain sufficient contrast in both application themes.

---

# 16. SKILLS MANAGEMENT

## 16.1 Skills screen

Provide a dedicated Skills management destination.

Use a library-style interface.

## 16.2 Skills tabs

- Installed.
- Discover.
- Categories.
- Sources.

## 16.3 Skills functionality

Support:

- Skill discovery.
- Search.
- Category filtering.
- Source management.
- Skill details.
- Installation.
- Enabling.
- Disabling.
- Removal.
- Project assignment.

## 16.4 Global and project-level behavior

Skills are managed in a global library.

Individual projects must be able to enable, disable or assign relevant skills independently.

The interface must clearly distinguish a globally installed skill from a skill enabled for a specific project.

## 16.5 Skill details

Selecting a skill should reveal a contextual details panel.

Include relevant information, installation status, source information and available actions.

---

# 17. PLUGIN MANAGEMENT

## 17.1 Plugins screen

Provide a dedicated plugin marketplace and management destination.

## 17.2 Plugin tabs

- Installed.
- Discover.
- Updates.
- Sources.

## 17.3 Plugin sources

Support:

- Official sources.
- Community sources.
- Custom URLs.
- Local plugin folders.

## 17.4 Plugin functionality

Support:

- Searching.
- Filtering.
- Discovering.
- Installing.
- Updating.
- Disabling.
- Removing.
- Inspecting details.
- Managing sources.

## 17.5 Plugin details

Selecting a plugin should reveal a details panel containing relevant information, source, version, status and available actions.

Do not treat plugin installation as equivalent to successful connection or configuration.

Clearly communicate required setup.

---

# 18. AUTOMATION MANAGEMENT

## 18.1 Automation workspace

Automation is a global management destination.

It must also support project-specific automations.

Global automation templates can be assigned to projects.

## 18.2 Automation tabs

The Automation workspace contains:

1. All Automations.
2. Templates.
3. Runs.
4. Schedules.
5. Integrations.

The visual workflow builder is a separate editor destination reachable from this workspace.

## 18.3 Automation capabilities

Support:

- Creating automations.
- Editing automations.
- Duplicating automations.
- Enabling automations.
- Disabling automations.
- Deleting automations.
- Assigning automations to projects.
- Managing templates.
- Inspecting execution history.
- Monitoring live executions.

## 18.4 Automation triggers

Support the following trigger categories:

- Manual.
- Scheduled.
- Task lifecycle.
- File changes.
- Git events.
- External webhooks.
- Integrations.

## 18.5 Automation builder

Provide a visual workflow builder.

Users should be able to create connected workflow nodes.

Provide simple automation templates to reduce setup complexity.

The builder should clearly display:

- Trigger.
- Conditions where supported.
- Actions.
- Connections.
- Execution order.
- Configuration.
- Validation errors.

## 18.6 Automation execution behavior

Once an automation has been enabled, it should run automatically when its configured trigger occurs.

Do not require the user to manually approve every normal automation execution.

Users must retain controls to disable, stop or otherwise manage automation behavior.

## 18.7 Automation monitoring

Provide:

- Run history.
- Detailed run pages.
- Live execution monitoring.

A run detail destination should expose:

- Trigger information.
- Execution status.
- Workflow steps.
- Logs.
- Start and completion information.
- Errors.
- Outcome.

The live automation monitor should show the current execution state.

Automation monitoring must remain distinct from the live task execution graph.

---

# 19. WORKSPACE MANAGEMENT

## 19.1 Multiple workspaces

Archify supports multiple workspaces.

Provide a workspace switcher.

## 19.2 Workspace management

Users must be able to:

- Add workspaces.
- Switch workspaces.
- Rename workspaces.
- Set a default workspace.
- Reopen the last-used workspace.
- Remove workspaces.
- Organize projects.
- Back up workspaces.
- Relocate workspaces.

## 19.3 Workspace settings

Workspace-related settings must be accessible through Settings.

The dedicated Workspace Manager should handle the broader management experience.

Do not confuse a workspace with an individual project.

---

# 20. PROVIDER AND AI CONNECTION MANAGEMENT

## 20.1 AI Connections screen

Provide a central overview of connected AI providers and services.

Users must be able to inspect connection status and configure providers individually.

## 20.2 Supported connection types

Support:

- API keys.
- Custom base URLs.
- Local model endpoints.
- Installed CLI agents.

## 20.3 Provider configuration

Individual provider configuration must support advanced options.

Include:

- Credentials.
- Base URL.
- Connection type.
- Available models.
- Request headers.
- Environment variables.
- Request parameters.
- Connection testing.
- Connection status.

## 20.4 Credential security

Use secure operating-system credential storage.

Credentials must:

- Be masked by default.
- Never be displayed unnecessarily.
- Be explicitly replaceable.
- Be explicitly removable.
- Never be exposed in ordinary logs.
- Never be accidentally included in public project exports.

The interface must communicate connection failures clearly without exposing secrets.

---

# 21. MODEL CATALOG AND MODEL PROFILES

## 21.1 Unified model catalog

Models must be presented in one unified catalog across connected providers.

Provide:

- Search.
- Filters.
- Provider filtering.
- Model details.

Do not restrict model discovery to individual provider configuration pages.

## 21.2 Model assignment

Users must be able to assign models independently to:

- Agents.
- Tasks.
- Conversations.

The interface should make the current assignment clear.

## 21.3 Model profiles

Support named, reusable model profiles.

Users can create profiles and assign them to:

- Agents.
- Tasks.
- Conversations.

The profile system should provide a consistent way to reuse model configurations.

---

# 22. NOTIFICATIONS

## 22.1 Notification architecture

Archify must provide two complementary notification systems:

1. Non-blocking toast notifications.
2. Persistent Notification Center.

## 22.2 Notification Center categories

Provide these categories:

- All.
- Unread.
- Errors.
- Warnings.
- Tasks.
- Automations.
- System.

## 22.3 Notification events

Support notifications for:

- Task started.
- Task completed.
- Task failed.
- Errors.
- Warnings.
- Agent issues.
- Required user attention.
- Automation events.
- System events.

## 22.4 Notification configuration

The Notifications Settings destination must allow independent configuration for every individual event type.

Do not reduce the entire notification system to one global on/off switch.

## 22.5 Notification presentation

Notifications should be:

- Non-blocking.
- Readable.
- Actionable where appropriate.
- Consistent with the theme.
- Dismissible where appropriate.

The persistent notification center must retain relevant notifications after their toast disappears.

---

# 23. ACTIVITY AND REPORTS

## 23.1 Global Activity

Provide a global Activity destination.

It must display application-wide activity across projects.

Users should be able to filter by project.

## 23.2 Project Activity

Every project also has its own Activity destination.

This view should focus on events within that project.

## 23.3 Global Reports

Provide a global Reports destination.

Users can inspect reports across projects and filter them by project.

## 23.4 Project Reports

Each project has a dedicated Reports destination.

## 23.5 Report presentation

Reports should be structured, readable and useful.

Support Markdown and PDF report presentation/export where appropriate.

Do not present generated reports as verified facts unless their underlying results have actually been checked.

---

# 24. SETTINGS ARCHITECTURE

The Settings area must use separate settings destinations.

The exact destinations are:

1. General.
2. Appearance.
3. Workspaces.
4. AI Connections.
5. Models.
6. Agents.
7. Notifications.
8. Privacy.
9. Import & Export.
10. About.

## 24.1 General

General application preferences and behavior.

## 24.2 Appearance

Provide:

- Dark Red theme.
- Light theme.
- Theme presets.
- Comfortable density.
- Compact density.
- Focus mode.
- Small, Medium and Large font presets.
- Sidebar behavior.
- Supported panel defaults.
- Advanced layout controls.

Avoid overwhelming users with unnecessary low-level visual settings.

## 24.3 Workspaces

Workspace-related preferences, default workspace and reopen-last behavior.

## 24.4 AI Connections

Provider overview and connection management.

## 24.5 Models

Unified model catalog and model management.

## 24.6 Agents

Agent management and configuration.

## 24.7 Notifications

Individual event-type notification controls.

## 24.8 Privacy

Privacy and data handling preferences.

Telemetry and crash reporting must be opt-in.

Do not silently collect telemetry or crash information.

## 24.9 Import & Export

Provide separate destinations or clearly separated experiences for:

- Full backup.
- Selective export.
- Selective import.

## 24.10 About

Application identity, version and relevant application information.

---

# 25. IMPORT AND EXPORT

## 25.1 Full backup

Support complete configuration backup and restoration.

## 25.2 Selective export

Allow users to select configuration categories for export.

## 25.3 Selective import

Allow users to import selected categories.

## 25.4 Conflict handling

The import experience must provide:

- Preview.
- Category selection.
- Conflict detection.
- Conflict resolution.
- Clear confirmation.
- Meaningful error messages.

Never silently overwrite existing configuration.

Credentials must receive special handling and must not be exposed through ordinary exports.

---

# 26. FIRST-LAUNCH EXPERIENCE

## 26.1 Setup wizard

New users should see a guided setup experience.

The wizard should cover:

1. Initial welcome.
2. Workspace setup.
3. Agent setup.
4. Provider configuration.
5. First project creation.
6. Initial readiness checks.

## 26.2 Skip option

Users must be able to skip onboarding.

Do not force users through every configuration step before they can explore the application.

## 26.3 Setup checklist

Skipped or incomplete setup tasks should remain accessible through a persistent setup checklist.

Users should be able to resume setup later.

The checklist should communicate which parts are complete, incomplete or require attention.

## 26.4 First-use experience

Empty states should explain:

- What the section is for.
- Why it is currently empty.
- What the user can do next.

Avoid blank screens with no explanation.

---

# 27. PERMISSIONS AND USER CONTROL

## 27.1 Global autonomy settings

Provide global autonomy settings.

These determine the default permission behavior of agents.

## 27.2 Per-agent overrides

Users must be able to configure permissions for individual agents.

## 27.3 Per-project overrides

Users must be able to override relevant permissions for individual projects.

## 27.4 Permission hierarchy

The interface must make it clear when a permission comes from:

- Global settings.
- Agent-specific configuration.
- Project-specific configuration.

Avoid confusing or contradictory permission displays.

## 27.5 Execution control

Users must be able to understand and control agent execution.

Provide appropriate actions to:

- Stop.
- Interrupt.
- Resume.
- Retry.
- Approve sensitive actions where required.

The interface should clearly distinguish normal automatic execution from actions that require user approval.

---

# 28. DIALOGS, MENUS AND OVERLAYS

These are interaction surfaces, not separate application screens.

## 28.1 Project dialogs

- Create project.
- Rename project.
- Duplicate project.
- Archive project.
- Remove project.
- Choose whether project files are retained or deleted.

## 28.2 Task dialogs

- Create task.
- Edit task.
- Stop execution.
- Retry task.
- Resume task.
- Confirm destructive actions.

## 28.3 Conversation dialogs

- New conversation.
- Rename conversation.
- Move conversation.
- Delete conversation.

## 28.4 Agent dialogs

- Agent selection.
- Permission approval.
- Execution interruption.

## 28.5 Provider dialogs

- Add provider.
- Test connection.
- Replace credential.
- Remove credential.

## 28.6 Model dialogs

- Assign model.
- Create profile.
- Edit profile.
- Delete profile.

## 28.7 Skills and plugin dialogs

- Install.
- Enable.
- Disable.
- Remove.
- Configure sources.
- Confirm relevant actions.

## 28.8 Automation dialogs

- Create automation.
- Enable.
- Disable.
- Run manually.
- Delete.
- Configure webhook.
- Confirm relevant execution actions.

## 28.9 Workspace dialogs

- Add.
- Rename.
- Switch.
- Relocate.
- Remove.

## 28.10 Import/export dialogs

- File selection.
- Category selection.
- Preview.
- Conflict resolution.
- Confirmation.

## 28.11 Global overlays

- Universal search.
- Command palette.
- Quick project switcher.
- Workspace switcher.
- Keyboard shortcut reference.
- Global quick-create menu.

---

# 29. INTERACTION AND ANIMATION SYSTEM

## 29.1 General interaction philosophy

Every interactive component should provide clear feedback.

Users should understand:

- What is clickable.
- What is selected.
- What is loading.
- What is executing.
- What is disabled.
- What has succeeded.
- What has failed.

## 29.2 Animation

Use subtle, intentional animations.

Appropriate examples:

- Sidebar transitions.
- Panel expansion and collapse.
- Tab transitions.
- Dialog appearance.
- Dropdown appearance.
- Toast notifications.
- Graph state transitions.
- Loading indicators.
- Navigation transitions.

Avoid excessive animations that make the application feel slow or distracting.

## 29.3 Interaction consistency

Buttons, menus, inputs, cards and controls must behave consistently throughout the application.

Destructive actions must be distinguishable from ordinary actions.

Hover, focus, active, selected, disabled and loading states must be designed rather than left to browser defaults.

---

# 30. RESPONSIVENESS AND LAYOUT ADAPTATION

Archify is primarily a desktop application.

Nevertheless, the interface must respond intelligently to different window sizes.

Support:

- Large desktop windows.
- Standard laptop windows.
- Smaller desktop windows.
- Resized application windows.

Adapt:

- Sidebar presentation.
- Main content width.
- Dashboard grid.
- Graph viewport.
- Details panels.
- Terminal panel.
- Repository panel.
- Tables and data-heavy sections.

Do not simply shrink the entire interface until text and controls become unusable.

---

# 31. EMPTY, LOADING, ERROR AND SUCCESS STATES

Every major screen and component must have intentional states.

## 31.1 Empty states

Examples:

- No projects.
- No tasks.
- No conversations.
- No agents configured.
- No connected providers.
- No models available.
- No installed skills.
- No plugins.
- No automations.
- No automation runs.
- No notifications.
- No reports.
- No recovery items.

Each empty state should explain the situation and provide a useful next action.

## 31.2 Loading states

Use clear loading feedback for:

- Project loading.
- Conversation loading.
- Provider discovery.
- Model discovery.
- Skills discovery.
- Plugin discovery.
- Automation execution.
- Report generation.
- Search.
- Import/export.

Avoid blank content areas that appear broken.

## 31.3 Error states

Error messages should explain:

- What failed.
- Which operation was affected.
- Whether data was preserved.
- Whether retry is possible.
- What the user can do next.

Avoid vague errors such as "Something went wrong" without supporting context.

## 31.4 Success states

Success feedback must only appear after the underlying operation succeeds.

Examples:

- Project created.
- Provider connected.
- Skill installed.
- Plugin updated.
- Automation enabled.
- Backup completed.
- Import completed.
- Task verified.

Never show success simply because a user clicked a button.

---

# 32. ACCESSIBILITY AND USABILITY

The interface should support:

- Readable text.
- Adequate contrast.
- Keyboard navigation.
- Visible focus indicators.
- Consistent interaction behavior.
- Clear labels.
- Meaningful icons.
- Tooltips where useful.
- Appropriate click targets.
- Reduced reliance on color alone.

Status must not be communicated exclusively through red, green or other colors.

Graph nodes, notification states and task statuses should also have text, symbols or other distinguishable visual cues.

---

# 33. PERFORMANCE AND VISUAL STABILITY

The interface should feel responsive and stable.

Design and implementation should prioritize:

- Efficient rendering.
- Avoiding unnecessary component re-renders.
- Efficient graph updates.
- Stable panel resizing.
- Smooth scrolling.
- Efficient large lists.
- Efficient search interactions.
- Responsive navigation.
- No unnecessary animations.
- No layout shifts during normal interaction.

Do not sacrifice functionality or readability just to make an animation look impressive.

---

# 34. VISUAL QUALITY REQUIREMENTS

This section is particularly important.

Archify must receive a genuine visual redesign.

Applying the crimson color palette to an existing interface is not an acceptable redesign.

The finished interface must demonstrate:

- A distinctive visual identity.
- A coherent design system.
- Consistent spacing.
- Consistent typography.
- Carefully designed surfaces.
- Refined borders and separators.
- Consistent corner radii.
- High-quality icons.
- Purposeful use of accent colors.
- Well-designed navigation.
- Thoughtful content hierarchy.
- Consistent component behavior.
- Professional empty states.
- Professional loading states.
- Professional error states.
- Cohesive light and dark themes.

Every major screen should feel like it belongs to the same product.

Avoid creating individual screens that look like unrelated templates.

---

# 35. EXACT SCREEN ARCHITECTURE

The approved architecture distinguishes screens from tabs, subpages, panels and overlays.

## A. Global application screens

1. Overview.
2. Skills.
3. Plugins.
4. Automation.
5. Projects.
6. Global Agent Console.
7. Global Activity.
8. Global Reports.
9. Recovery Center.
10. Notification Center.
11. Workspace Manager.
12. Archive.
13. Trash.

## B. Project workspace screens

14. Project Playground.
15. Project Tasks.
16. Project Conversations.
17. Project Agent Console.
18. Project Map.
19. Project Activity.
20. Project Reports.
21. Project Settings.

## C. Settings screens

22. General.
23. Appearance.
24. Workspaces.
25. AI Connections.
26. Models.
27. Agents.
28. Notifications.
29. Privacy.
30. Import & Export.
31. About.

## D. Additional navigable screens

32. Task Detail.
33. Conversation Detail.
34. Provider Configuration.
35. Model Details.
36. Model Profile Editor.
37. Individual Agent Configuration.
38. Automation Builder.
39. Automation Details.
40. Automation Run Details.
41. Live Automation Monitor.
42. Global Search Results.
43. Full Backup.
44. Selective Export.
45. Selective Import.
46. First-launch Setup Wizard.
47. Setup Checklist.

**Total architectural screen templates: 47.**

This number represents the defined screen templates, not every instance of a project, task, provider, model, agent, automation or report.

## E. Internal tabs

Playground:
- Graph.
- Chat.

Skills:
- Installed.
- Discover.
- Categories.
- Sources.

Plugins:
- Installed.
- Discover.
- Updates.
- Sources.

Automation:
- All Automations.
- Templates.
- Runs.
- Schedules.
- Integrations.

Notification Center:
- All.
- Unread.
- Errors.
- Warnings.
- Tasks.
- Automations.
- System.

## F. Persistent panels

Primary panels:

1. Global Sidebar.
2. Repository Panel.
3. Terminal Panel.
4. Contextual Details Inspector.
5. Status and Notification Surface.

Additional contextual panels:

- Task Recovery.
- Agent Activity Details.
- Skill Details.
- Plugin Details.
- Automation Inspection.
- Project Navigation Expansion.
- Conversation Context and Attachments.

## G. Global overlays

1. Universal Search.
2. Command Palette.
3. Quick Project Switcher.
4. Workspace Switcher.
5. Keyboard Shortcut Reference.
6. Global Quick-create Menu.

---

# 36. IMPLEMENTATION PRIORITIES

Implement the interface in deliberate stages.

## Stage 1 — Design system and application shell

- Color tokens.
- Typography.
- Spacing.
- Component foundations.
- Theme system.
- Sidebar.
- Main layout.
- Panel architecture.
- Navigation.
- Responsive behavior.
- Keyboard interaction foundations.

## Stage 2 — Main global destinations

- Overview.
- Projects.
- Skills.
- Plugins.
- Automation.
- Activity.
- Reports.
- Notifications.

## Stage 3 — Project workspace

- Playground.
- Graph.
- Chat.
- Tasks.
- Task Detail.
- Conversations.
- Agent Console.
- Project Map.
- Project Settings.

## Stage 4 — Agent execution and monitoring

- Global Agent Console.
- Project Agent Console.
- Execution states.
- Live graph.
- Task lifecycle.
- Recovery Center.
- Task-level recovery.

## Stage 5 — AI configuration

- AI Connections.
- Provider configuration.
- Model catalog.
- Model details.
- Model profiles.
- Agent configuration.
- Permission controls.

## Stage 6 — Automation

- Automation workspace.
- Templates.
- Workflow builder.
- Integrations.
- Schedules.
- Run history.
- Run details.
- Live monitoring.

## Stage 7 — Supporting functionality

- Workspace Manager.
- Archive.
- Trash.
- Import/export.
- First-launch wizard.
- Setup checklist.
- Search.
- Command palette.
- Remaining settings.

## Stage 8 — Quality assurance

- Full screen-by-screen visual inspection.
- Navigation testing.
- Responsive layout testing.
- Theme testing.
- Keyboard interaction testing.
- Empty-state testing.
- Loading-state testing.
- Error-state testing.
- Real execution-state testing.
- Graph interaction testing.
- Terminal and repository panel testing.
- Dialog and overlay testing.
- Regression testing.

---

# 37. NON-NEGOTIABLE DESIGN RULES

1. Do not redesign Archify by simply applying red colors to the old interface.
2. Do not make the application look like a generic dashboard template.
3. Do not sacrifice usability for visual decoration.
4. Do not combine the Live Execution Graph and Project Map into one screen.
5. Do not turn the Graph and Chat tabs into separate Playground screens.
6. Do not make the repository tree a code editor.
7. Do not add a full built-in file-management or Git interface.
8. Do not allow freeform rearrangement of the main application panels.
9. Dashboard widgets must remain independently movable and resizable.
10. The terminal must be collapsed by default.
11. The repository tree must be read-only.
12. Multiple projects must be able to execute concurrently.
13. A project must not execute more than one task at a time.
14. Global and project-level monitoring must both exist.
15. Global and task-level recovery must both exist.
16. Automation must execute automatically after being enabled and triggered.
17. Do not require manual approval for every normal automation run.
18. Global, agent and project permission settings must be clearly distinguished.
19. Do not fabricate execution progress or task completion.
20. Do not expose credentials in logs or ordinary exports.
21. Do not silently overwrite configuration during imports.
22. Telemetry and crash reporting must remain opt-in.
23. All major screens must have designed empty, loading, error and success states.
24. All major screens must follow the same design system.
25. The Light theme must receive the same design attention as Dark Red.
26. Do not claim a screen or feature is complete based only on a successful build.
27. Every major screen must receive actual visual and interaction QA.
28. Preserve the existing application's underlying functionality when implementing the visual redesign.
29. Do not introduce placeholder interactions that look functional but do nothing.
30. Do not mark an operation successful before its underlying operation has actually succeeded.

---

# 38. FINAL DESIGN OBJECTIVE

Archify should ultimately feel like a premium AI development command center.

A user should be able to open the application and immediately understand:

- What projects exist.
- Which projects are active.
- Which agents are working.
- Which tasks are executing.
- What each agent is doing.
- What has completed.
- What has failed.
- What requires attention.
- Which automations are running.
- How their AI providers and models are configured.
- What is happening inside each individual project.

The application should make complex AI-assisted development workflows understandable and manageable through a polished, coherent and highly functional desktop experience.

**The target is not merely a beautiful interface. The target is a beautiful, powerful, consistent and genuinely usable product.**