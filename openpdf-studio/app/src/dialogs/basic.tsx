import { useEffect, useState } from "react";
import { ExternalLink, KeyRound, Keyboard, Settings as SettingsIcon, Sparkles } from "lucide-react";
import { Button, Checkbox, Field, Modal, Segmented, Kbd } from "../components/ui";
import { closeDialog, set, toast, useStore, type ThemePref } from "../state/store";
import { commands } from "../state/commands";
import { api } from "../lib/ipc";
import type { AppInfo } from "../lib/types";
import { saveSettings } from "../state/settings";

export function PasswordDialog({ fileName, retry }: { fileName: string; retry: boolean }) {
  const [pw, setPw] = useState("");
  const [show, setShow] = useState(false);
  return (
    <Modal
      title="Password required"
      icon={<KeyRound size={18} />}
      onClose={() => closeDialog(null)}
      footer={
        <>
          <Button onClick={() => closeDialog(null)}>Cancel</Button>
          <Button variant="primary" disabled={!pw} onClick={() => closeDialog(pw)}>
            Open
          </Button>
        </>
      }
    >
      <p>
        “{fileName}” is protected. Enter its password to open it. The password is used only on this computer and is never stored.
      </p>
      <Field label="Password" error={retry ? "That password is not correct. Try again." : null}>
        <input
          className="input"
          type={show ? "text" : "password"}
          value={pw}
          data-autofocus
          autoComplete="off"
          onChange={(e) => setPw(e.target.value)}
          onKeyDown={(e) => e.key === "Enter" && pw && closeDialog(pw)}
        />
      </Field>
      <Checkbox checked={show} onChange={setShow} label="Show password" />
    </Modal>
  );
}

export function ConfirmDialog({ title, body, actions }: { title: string; body: string; actions: { id: string; label: string; primary?: boolean; danger?: boolean }[] }) {
  return (
    <Modal
      title={title}
      onClose={() => closeDialog(null)}
      footer={actions.map((a) => (
        <Button key={a.id} variant={a.primary ? "primary" : a.danger ? "danger" : "default"} data-autofocus={a.primary ? true : undefined} onClick={() => closeDialog(a.id)}>
          {a.label}
        </Button>
      ))}
    >
      <p>{body}</p>
    </Modal>
  );
}

export function LinkDialog({ url }: { url: string }) {
  const display = url.length > 300 ? url.slice(0, 300) + "…" : url;
  return (
    <Modal
      title="Open external link?"
      icon={<ExternalLink size={18} />}
      onClose={() => closeDialog()}
      footer={
        <>
          <Button onClick={() => closeDialog()}>Cancel</Button>
          <Button
            onClick={() => {
              void navigator.clipboard.writeText(url).then(() => toast("success", "Link copied"));
              closeDialog();
            }}
          >
            Copy link
          </Button>
          <Button
            variant="primary"
            data-autofocus
            onClick={() => {
              closeDialog();
              api.openExternal(url).catch((e) => toast("error", "Could not open the link", String(e.message ?? e)));
            }}
          >
            Open in browser
          </Button>
        </>
      }
    >
      <p>This document links to a location outside OpenPDF Studio. Only open links you trust.</p>
      <div className="callout" style={{ fontFamily: "var(--mono)", wordBreak: "break-all" }}>
        {display}
      </div>
    </Modal>
  );
}

export function ShortcutsDialog() {
  const groups: [string, string[]][] = [
    ["File", ["open", "save", "saveAs", "print", "close", "properties"]],
    ["Edit", ["undo", "redo", "find", "rotateRight", "rotateLeft"]],
    ["View", ["zoomIn", "zoomOut", "fitPage", "fitWidth", "actualSize", "sidebar", "organize", "nextPage", "prevPage", "firstPage", "lastPage", "nextTab", "prevTab"]],
    ["Comment tools", ["toolSelect", "toolHighlight", "toolText", "toolInk"]],
    ["App", ["shortcuts", "settings"]],
  ];
  const byId = Object.fromEntries(commands.map((c) => [c.id, c]));
  return (
    <Modal title="Keyboard shortcuts" icon={<Keyboard size={18} />} onClose={() => closeDialog()} width={560}>
      <div className="shortcut-table">
        {groups.map(([g, ids]) => (
          <div key={g} style={{ display: "contents" }}>
            <h3>{g}</h3>
            {ids.map((id) => {
              const c = byId[id];
              if (!c?.shortcut) return null;
              return (
                <div key={id} style={{ display: "contents" }}>
                  <span>{c.label}</span>
                  <span>
                    <Kbd combo={c.shortcut} />
                  </span>
                </div>
              );
            })}
          </div>
        ))}
        <h3>Pages (sidebar and grid)</h3>
        <span>Select several pages</span>
        <span>
          <Kbd combo="Mod+Click" /> / <Kbd combo="Shift+Click" />
        </span>
        <span>Move selected pages</span>
        <span>
          Drag, or <Kbd combo="Alt+ArrowLeft" /> <Kbd combo="Alt+ArrowRight" />
        </span>
        <span>Delete selected pages</span>
        <span>
          <Kbd combo="Delete" />
        </span>
      </div>
    </Modal>
  );
}

export function SettingsDialog() {
  const theme = useStore((s) => s.theme);
  const recovery = useStore((s) => s.recovery);
  const [info, setInfo] = useState<AppInfo | null>(null);
  useEffect(() => {
    api.appInfo().then(setInfo).catch(() => {});
  }, []);
  return (
    <Modal title="Settings" icon={<SettingsIcon size={18} />} onClose={() => closeDialog()} width={520} footer={<Button onClick={() => closeDialog()}>Done</Button>}>
      <Field label="Appearance">
        <Segmented<ThemePref>
          label="Theme"
          value={theme}
          onChange={(t) => {
            set({ theme: t });
            void saveSettings();
          }}
          options={[
            { value: "system", label: "System" },
            { value: "light", label: "Light" },
            { value: "dark", label: "Dark" },
          ]}
        />
      </Field>
      <Field
        label="Crash recovery"
        hint={
          <>
            Unsaved work is kept in a private folder on this computer and offered back after a crash. Password-protected documents stay encrypted
            there. {info && <>Location: {info.recoveryDir}</>}
          </>
        }
      >
        <Checkbox
          checked={recovery}
          onChange={(v) => {
            set({ recovery: v });
            void saveSettings();
            toast("info", v ? "Crash recovery enabled" : "Crash recovery disabled", "Applies to documents opened from now on.");
          }}
          label="Keep recovery copies of unsaved changes"
        />
      </Field>
      <Field label="Recent files">
        <div>
          <Button
            size="sm"
            onClick={async () => {
              await api.forgetRecent(null);
              toast("success", "Recent files cleared");
            }}
          >
            Clear recent files
          </Button>
        </div>
      </Field>
      <div className="callout">
        OpenPDF Studio makes no network connections on its own. Links in documents open in your browser only after you confirm.
      </div>
    </Modal>
  );
}

export function AboutDialog() {
  const [info, setInfo] = useState<AppInfo | null>(null);
  useEffect(() => {
    api.appInfo().then(setInfo).catch(() => {});
  }, []);
  return (
    <Modal title="About OpenPDF Studio" icon={<Sparkles size={18} />} onClose={() => closeDialog()} footer={<Button onClick={() => closeDialog()}>Close</Button>}>
      <p>
        Version {info?.version ?? "…"} · {info?.platform}
        <br />
        Free and open source under the Mozilla Public License 2.0. No accounts, ads, watermarks, telemetry or usage limits.
      </p>
      <div className="callout">
        Built on pdf.js (Apache-2.0) for rendering, forms and annotations; lopdf (MIT) for document structure, encryption and saving; Tauri (MIT/Apache-2.0),
        React (MIT), image-rs (MIT/Apache-2.0) and Lucide icons (ISC). See THIRD_PARTY_NOTICES.md for details.
      </div>
    </Modal>
  );
}

export function useThemeEffect() {
  const theme = useStore((s) => s.theme);
  useEffect(() => {
    const mq = window.matchMedia("(prefers-color-scheme: dark)");
    const apply = () => {
      const dark = theme === "dark" || (theme === "system" && mq.matches);
      document.documentElement.dataset.theme = dark ? "dark" : "light";
    };
    apply();
    mq.addEventListener("change", apply);
    return () => mq.removeEventListener("change", apply);
  }, [theme]);
}

