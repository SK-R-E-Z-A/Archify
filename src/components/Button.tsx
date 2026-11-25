import React from "react";

export type ButtonRole = "primary" | "secondary" | "danger" | "ghost";
export type ButtonSize = "sm" | "md" | "lg";
export type ButtonVariant = "fill" | "outline" | "ghost";

interface ButtonProps extends React.ButtonHTMLAttributes<HTMLButtonElement> {
  /** Visual role of the button. Primary = most likely action (one or two per view). */
  role?: ButtonRole;
  /** Size variant. */
  size?: ButtonSize;
  /** Whether the button spans full width of its container. */
  block?: boolean;
  /** Icon-only square button. */
  icon?: boolean;
  /** Show a loading spinner and disable interaction. */
  loading?: boolean;
  /** Compact density override. */
  compact?: boolean;
  /** Tooltip-style title for icon buttons. */
  title?: string;
  children?: React.ReactNode;
}

const ROLE_CLASS: Record<ButtonRole, string> = {
  primary: "btn-primary",
  secondary: "btn-secondary",
  danger: "btn-danger",
  ghost: "btn-ghost",
};

const SIZE_CLASS: Record<ButtonSize, string> = {
  sm: "btn-sm",
  md: "",
  lg: "btn-lg",
};

/**
 * Archify Button — responsive, role-based, HIG-aligned.
 *
 * - Press state on :active (§buttons.md: required for custom buttons)
 *   - Accessible hit region ≥ 28×28 desktop
 *   - Visible focus-visible ring for keyboard navigation
 *   - Scales with font-size presets + density presets
 *   - Responsive: stacks groups on narrow widths
 */
export function Button({
  role = "primary",
  size = "md",
  block = false,
  icon = false,
  loading = false,
  compact = false,
  title,
  className = "",
  disabled,
  children,
  ...props
}: ButtonProps) {
  const classes = [
    "btn",
    ROLE_CLASS[role],
    size !== "md" ? SIZE_CLASS[size] : "",
    block ? "btn-block" : "",
    icon ? "btn-icon" : "",
    loading ? "btn-loading" : "",
    compact ? "compact" : "",
    className,
  ]
    .filter(Boolean)
    .join(" ");

  return (
    <button
      type="button"
      className={classes}
      disabled={disabled || loading}
      title={title}
      aria-busy={loading || undefined}
      {...props}
    >
      {children}
    </button>
  );
}

export default Button;
