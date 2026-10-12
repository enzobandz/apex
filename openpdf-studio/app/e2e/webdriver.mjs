// Minimal W3C WebDriver client (no dependencies) for tauri-driver.

const ELEMENT = "element-6066-11e4-a52e-4f735466cecf";

export class Session {
  constructor(base, id) {
    this.base = base;
    this.id = id;
  }

  static async create(base, application, args = []) {
    const res = await call(base, "POST", "/session", {
      capabilities: { alwaysMatch: { "tauri:options": { application, args } } },
    });
    return new Session(base, res.value?.sessionId ?? res.sessionId);
  }

  cmd(method, path, body) {
    return call(this.base, method, `/session/${this.id}${path}`, body);
  }

  /** Run a script in the page; `args` are passed as `arguments`. Supports async (return a promise). */
  async exec(script, ...args) {
    return (await this.cmd("POST", "/execute/async", {
      script: `const done = arguments[arguments.length - 1];
               Promise.resolve((async (...__args) => { ${script} })(...Array.from(arguments).slice(0, -1)))
                 .then(v => done({ ok: true, v }), e => done({ ok: false, e: String(e) + (e && e.stack ? ' | ' + e.stack : '') }));`,
      args,
    })).value;
  }

  async run(script, ...args) {
    const r = await this.exec(script, ...args);
    if (!r?.ok) throw new Error(`script failed: ${r?.e}`);
    return r.v;
  }

  async waitFor(script, { timeout = 15000, interval = 150, message = script } = {}, ...args) {
    const end = Date.now() + timeout;
    let last;
    while (Date.now() < end) {
      const r = await this.exec(script, ...args);
      if (r?.ok && r.v) return r.v;
      last = r;
      await new Promise((res) => setTimeout(res, interval));
    }
    throw new Error(`timed out waiting for: ${message} (last: ${JSON.stringify(last)})`);
  }

  async find(css) {
    const r = await this.cmd("POST", "/element", { using: "css selector", value: css });
    return r.value[ELEMENT];
  }

  async click(css) {
    const el = await this.find(css);
    await this.cmd("POST", `/element/${el}/click`, {});
  }

  async type(css, text) {
    const el = await this.find(css);
    await this.cmd("POST", `/element/${el}/value`, { text });
  }

  async keys(text) {
    const actions = [...text].flatMap((c) => [{ type: "keyDown", value: c }, { type: "keyUp", value: c }]);
    await this.cmd("POST", "/actions", { actions: [{ type: "key", id: "kbd", actions }] });
  }

  /** Pointer click at viewport coordinates. */
  async clickAt(x, y) {
    await this.cmd("POST", "/actions", {
      actions: [
        {
          type: "pointer",
          id: "mouse",
          parameters: { pointerType: "mouse" },
          actions: [
            { type: "pointerMove", duration: 0, x: Math.round(x), y: Math.round(y) },
            { type: "pointerDown", button: 0 },
            { type: "pointerUp", button: 0 },
          ],
        },
      ],
    });
    await this.cmd("DELETE", "/actions");
  }

  async drag(points) {
    const [first, ...rest] = points;
    await this.cmd("POST", "/actions", {
      actions: [
        {
          type: "pointer",
          id: "mouse",
          parameters: { pointerType: "mouse" },
          actions: [
            { type: "pointerMove", duration: 0, x: Math.round(first[0]), y: Math.round(first[1]) },
            { type: "pointerDown", button: 0 },
            ...rest.map(([x, y]) => ({ type: "pointerMove", duration: 40, x: Math.round(x), y: Math.round(y) })),
            { type: "pointerUp", button: 0 },
          ],
        },
      ],
    });
    await this.cmd("DELETE", "/actions");
  }

  async screenshot() {
    return Buffer.from((await this.cmd("GET", "/screenshot")).value, "base64");
  }

  async close() {
    await call(this.base, "DELETE", `/session/${this.id}`).catch(() => {});
  }
}

async function call(base, method, path, body) {
  const res = await fetch(base + path, {
    method,
    headers: { "content-type": "application/json" },
    body: body === undefined ? undefined : JSON.stringify(body),
  });
  const json = await res.json().catch(() => ({}));
  if (!res.ok) throw new Error(`${method} ${path} → ${res.status}: ${JSON.stringify(json.value ?? json)}`);
  return json;
}
