import { cloneElement, isValidElement, useEffect, useId, useLayoutEffect, useRef, useState, type ReactNode, type ButtonHTMLAttributes } from "react";
import { createPortal } from "react-dom";
import { X } from "lucide-react";
import { label as keyLabel } from "../lib/keys";

type BtnProps = ButtonHTMLAttributes<HTMLButtonElement> & {
  variant?: "default" | "primary" | "subtle" | "danger";
  size?: "sm" | "md";
  icon?: ReactNode;
};

export function Button({ variant = "default", size = "md", icon, children, className = "", ...rest }: BtnProps) {
  return (
    <button type="button" className={`btn btn-${variant} btn-${size} ${className}`} {...rest}>
      {icon}
      {children != null && <span>{children}</span>}
    </button>
  );
}

/** Icon-only button with an accessible name and tooltip (incl. shortcut). */
export function IconButton({
  label,
  shortcut,
  active,
  className = "",
  children,
  ...rest
}: ButtonHTMLAttributes<HTMLButtonElement> & { label: string; shortcut?: string; active?: boolean }) {
  const tip = shortcut ? `${label} (${keyLabel(shortcut)})` : label;
  return (
    <button
      type="button"
      className={`icon-btn ${active ? "is-active" : ""} ${className}`}
      aria-label={label}
      aria-pressed={active === undefined ? undefined : active}
      title={tip}
      {...rest}
    >
      {children}
    </button>
  );
}

/** Large ribbon button: icon above label. */
export function ToolButton({
  icon,
  label,
  shortcut,
  active,
  ...rest
}: ButtonHTMLAttributes<HTMLButtonElement> & { icon: ReactNode; label: string; shortcut?: string; active?: boolean }) {
  return (
    <button
      type="button"
      className={`tool-btn ${active ? "is-active" : ""}`}
      title={shortcut ? `${label} (${keyLabel(shortcut)})` : label}
      aria-pressed={active === undefined ? undefined : active}
      {...rest}
    >
      <span className="tool-icon">{icon}</span>
      <span className="tool-label">{label}</span>
    </button>
  );
}

export function Kbd({ combo }: { combo: string }) {
  return <kbd className="kbd">{keyLabel(combo)}</kbd>;
}

export function Spinner({ size = 16 }: { size?: number }) {
  return <span className="spinner" style={{ width: size, height: size }} role="progressbar" aria-label="Working" />;
}

export function Field({ label, hint, children, error }: { label: string; hint?: ReactNode; error?: string | null; children: ReactNode }) {
  const id = useId();
  return (
    <div className={`field ${error ? "has-error" : ""}`}>
      <label htmlFor={id}>{label}</label>
      <div className="field-control" id={id + "-wrap"}>
        {withId(children, id)}
      </div>
      {error ? <div className="field-error">{error}</div> : hint ? <div className="field-hint">{hint}</div> : null}
    </div>
  );
}

function withId(children: ReactNode, id: string): ReactNode {
  if (isValidElement<{ id?: string }>(children) && !children.props.id) return cloneElement(children, { id });
  return children;
}

export function Checkbox({ checked, onChange, label, disabled }: { checked: boolean; onChange: (v: boolean) => void; label: ReactNode; disabled?: boolean }) {
  return (
    <label className={`checkbox ${disabled ? "is-disabled" : ""}`}>
      <input type="checkbox" checked={checked} disabled={disabled} onChange={(e) => onChange(e.target.checked)} />
      <span className="checkbox-box" aria-hidden />
      <span>{label}</span>
    </label>
  );
}

export function Segmented<T extends string>({
  value,
  options,
  onChange,
  label,
}: {
  value: T;
  options: { value: T; label: ReactNode; title?: string }[];
  onChange: (v: T) => void;
  label: string;
}) {
  return (
    <div className="segmented" role="radiogroup" aria-label={label}>
      {options.map((o) => (
        <button
          key={o.value}
          type="button"
          role="radio"
          aria-checked={o.value === value}
          title={o.title}
          className={o.value === value ? "is-active" : ""}
          onClick={() => onChange(o.value)}
        >
          {o.label}
        </button>
      ))}
    </div>
  );
}

const FOCUSABLE = 'button:not([disabled]), [href], input:not([disabled]), select:not([disabled]), textarea:not([disabled]), [tabindex]:not([tabindex="-1"])';

/** Accessible modal dialog: focus trap, Escape closes, focus restored. */
export function Modal({
  title,
  onClose,
  children,
  footer,
  width = 480,
  icon,
}: {
  title: string;
  onClose: () => void;
  children: ReactNode;
  footer?: ReactNode;
  width?: number;
  icon?: ReactNode;
}) {
  const ref = useRef<HTMLDivElement>(null);
  const titleId = useId();
  useEffect(() => {
    const prev = document.activeElement as HTMLElement | null;
    const el = ref.current;
    const first = el?.querySelector<HTMLElement>("[data-autofocus]") ?? el?.querySelector<HTMLElement>(FOCUSABLE);
    first?.focus();
    const onKey = (e: KeyboardEvent) => {
      if (e.key === "Escape") {
        e.stopPropagation();
        onClose();
      } else if (e.key === "Tab" && el) {
        const items = [...el.querySelectorAll<HTMLElement>(FOCUSABLE)].filter((x) => x.offsetParent !== null);
        if (!items.length) return;
        const firstEl = items[0]!;
        const lastEl = items[items.length - 1]!;
        if (e.shiftKey && document.activeElement === firstEl) {
          e.preventDefault();
          lastEl.focus();
        } else if (!e.shiftKey && document.activeElement === lastEl) {
          e.preventDefault();
          firstEl.focus();
        }
      }
    };
    window.addEventListener("keydown", onKey, true);
    return () => {
      window.removeEventListener("keydown", onKey, true);
      prev?.focus?.();
    };
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, []);
  return createPortal(
    <div className="modal-backdrop" onMouseDown={(e) => e.target === e.currentTarget && onClose()}>
      <div className="modal" role="dialog" aria-modal="true" aria-labelledby={titleId} ref={ref} style={{ width }}>
        <div className="modal-header">
          {icon && <span className="modal-icon">{icon}</span>}
          <h2 id={titleId}>{title}</h2>
          <IconButton label="Close" onClick={onClose} className="modal-close">
            <X size={16} />
          </IconButton>
        </div>
        <div className="modal-body">{children}</div>
        {footer && <div className="modal-footer">{footer}</div>}
      </div>
    </div>,
    document.body,
  );
}

export interface MenuItem {
  label: string;
  icon?: ReactNode;
  shortcut?: string;
  disabled?: boolean;
  danger?: boolean;
  onSelect: () => void;
}

/** Context/dropdown menu positioned at a point, keyboard navigable. */
export function Menu({ x, y, items, onClose }: { x: number; y: number; items: (MenuItem | "-")[]; onClose: () => void }) {
  const ref = useRef<HTMLDivElement>(null);
  const [pos, setPos] = useState({ x, y });
  useLayoutEffect(() => {
    const el = ref.current;
    if (!el) return;
    const r = el.getBoundingClientRect();
    setPos({ x: Math.min(x, window.innerWidth - r.width - 8), y: Math.min(y, window.innerHeight - r.height - 8) });
    el.querySelector<HTMLElement>("button:not([disabled])")?.focus();
  }, [x, y]);
  useEffect(() => {
    const close = (e: Event) => {
      if (e instanceof KeyboardEvent) {
        if (e.key === "Escape") onClose();
        return;
      }
      if (!ref.current?.contains(e.target as Node)) onClose();
    };
    window.addEventListener("mousedown", close, true);
    window.addEventListener("keydown", close, true);
    window.addEventListener("blur", onClose);
    return () => {
      window.removeEventListener("mousedown", close, true);
      window.removeEventListener("keydown", close, true);
      window.removeEventListener("blur", onClose);
    };
  }, [onClose]);
  const onKeyDown = (e: React.KeyboardEvent) => {
    const btns = [...(ref.current?.querySelectorAll<HTMLButtonElement>("button:not([disabled])") ?? [])];
    const i = btns.indexOf(document.activeElement as HTMLButtonElement);
    if (e.key === "ArrowDown") {
      e.preventDefault();
      btns[(i + 1) % btns.length]?.focus();
    } else if (e.key === "ArrowUp") {
      e.preventDefault();
      btns[(i - 1 + btns.length) % btns.length]?.focus();
    }
  };
  return createPortal(
    <div className="menu" role="menu" ref={ref} style={{ left: pos.x, top: pos.y }} onKeyDown={onKeyDown}>
      {items.map((it, i) =>
        it === "-" ? (
          <div key={i} className="menu-sep" role="separator" />
        ) : (
          <button
            key={i}
            type="button"
            role="menuitem"
            disabled={it.disabled}
            className={it.danger ? "is-danger" : ""}
            onClick={() => {
              onClose();
              it.onSelect();
            }}
          >
            <span className="menu-icon">{it.icon}</span>
            <span className="menu-label">{it.label}</span>
            {it.shortcut && <span className="menu-shortcut">{keyLabel(it.shortcut)}</span>}
          </button>
        ),
      )}
    </div>,
    document.body,
  );
}

export function Empty({ icon, title, children }: { icon: ReactNode; title: string; children?: ReactNode }) {
  return (
    <div className="empty">
      <div className="empty-icon">{icon}</div>
      <div className="empty-title">{title}</div>
      {children && <div className="empty-body">{children}</div>}
    </div>
  );
}
