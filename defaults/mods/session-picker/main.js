// Directory-backed session picker for the C-a f workflow.
// Select a project folder, then focus its existing space or create one there.

import { View, div, svg } from "gpui-kit";
import { v_flex, h_flex, InputState, Input } from "gpui-base";
import { exec, home, spaces, run, theme, props, closeOverlay, shared, setShared } from "herdr";

const LIST_SCRIPT = [
  "set -f",
  "home=$1",
  "for spec in \"$home/tmp:0\" \"$home/dev:2\" \"$home/storage/projects:2\" \"$home/games:1\"; do",
  "  root=${spec%:*}; depth=${spec#*:}",
  "  if [ \"$depth\" -eq 0 ]; then",
  "    [ -d \"$root\" ] && printf '%s\\0' \"$root\"",
  "    continue",
  "  fi",
  "  [ -d \"$root\" ] || continue",
  "  find -L \"$root\" -mindepth 1 -maxdepth \"$depth\" \\",
  "    \\( -name '.*' -o -name node_modules -o -name dist -o -name build -o -name out \\",
  "       -o -name coverage -o -name target -o -name tmp -o -name cache -o -name venv \\",
  "       -o -name __pycache__ -o -name 'lost+found' \\) -prune -o -type d -print0 2>/dev/null || true",
  "done",
].join("\\n");

function displayPath(path) {
  const h = home();
  return path === h ? "~" : path.startsWith(`${h}/`) ? `~${path.slice(h.length)}` : path;
}

const HISTORY_KEY = "session-picker.history";
const HISTORY_LIMIT = 20;

function cleanHistory(value) {
  return Array.isArray(value) ? [...new Set(value.filter((path) => typeof path === "string" && path.length))].slice(0, HISTORY_LIMIT) : [];
}

function fuzzy(query, text) {
  let at = 0;
  let score = 0;
  let run = 0;
  for (const ch of query) {
    const i = text.indexOf(ch, at);
    if (i < 0) return null;
    const boundary = i === 0 || /[\s/_.-]/.test(text[i - 1]);
    run = i === at ? run + 1 : 0;
    score += 1 + (boundary ? 6 : 0) + run * 3 - Math.min(i - at, 6) * 0.3;
    at = i + 1;
  }
  return score - text.length * 0.02;
}

export default class SessionPicker extends View {
  init(_props, cx) {
    this.filter = "";
    this.sessions = [];
    this.history = cleanHistory(shared(HISTORY_KEY));
    this.error = null;
    this.at = 0;
    this.input = InputState.new({ placeholder: "Find a project directory or recent session…" });
    this.input.on("change", (_e, cx) => {
      this.filter = this.input.value();
      this.at = 0;
      cx.notify();
    });
    this.input.on("submit", () => this.go(this.rows()[this.at]));
    this.input.focus();
    cx.timer.after(600, () => this.input.focus());
    this.handle = cx.focus_handle();
    cx.spawn(async (cx) => {
      try {
        const result = await exec("sh", ["-c", LIST_SCRIPT, "herdr-session-picker", home()]);
        if (result.code !== 0) throw new Error(result.stderr || "Cannot list session directories");
        this.sessions = [...new Set(result.stdout.split("\0").filter(Boolean))].sort((a, b) => a.localeCompare(b));
        const available = new Set(this.sessions);
        const kept = this.history.filter((path) => available.has(path));
        if (kept.length !== this.history.length) {
          this.history = kept;
          setShared(HISTORY_KEY, kept);
        }
      } catch (error) {
        this.error = error?.message ?? "Cannot list session directories";
      }
      cx.notify();
    });
  }

  close() {
    closeOverlay(props().overlay);
  }

  rows() {
    const query = this.filter.trim().toLowerCase();
    const open = spaces().filter((s) => !s.machine);
    const recent = new Set(this.history);
    const paths = [
      ...this.history.filter((path) => this.sessions.includes(path)),
      ...this.sessions.filter((path) => !recent.has(path)),
    ];
    return paths
      .map((path) => {
        const display = displayPath(path);
        const space = open.find((s) => s.folder === path) ?? null;
        const text = `${display} ${space?.name ?? ""}`.toLowerCase();
        const match = query ? fuzzy(query, text) : 0;
        if (match === null) return null;
        const score = query ? match + (recent.has(path) ? 0.5 : 0) : recent.has(path) ? 1000 - this.history.indexOf(path) : 0;
        return { kind: "session", path, display, space, recent: recent.has(path), score };
      })
      .filter(Boolean)
      .sort((a, b) => b.score - a.score || a.display.localeCompare(b.display));
  }

  remember(path) {
    this.history = [path, ...this.history.filter((item) => item !== path)].slice(0, HISTORY_LIMIT);
    setShared(HISTORY_KEY, this.history);
  }

  go(row) {
    if (!row) return;
    this.remember(row.path);
    this.close();
    if (row.space) run("space.select", { id: row.space.id });
    else run("space.new", { folder: row.path, name: row.display.slice(2) || "tmp" });
  }

  render() {
    const t = theme();
    const rows = this.rows();
    this.at = Math.min(this.at, Math.max(0, rows.length - 1));
    const selected = rows[this.at];
    return v_flex()
      .id("session-picker")
      .track_focus(this.handle)
      .w_full()
      .min_h(0)
      .p(18)
      .gap(10)
      .bg(t.popover)
      .text_size(13)
      .on_key_down((e, cx) => {
        if (e.key === "escape") return this.close();
        if (!rows.length) return;
        if (e.key === "down") this.at = (this.at + 1) % rows.length;
        else if (e.key === "up") this.at = (this.at + rows.length - 1) % rows.length;
        else return;
        cx.stop_propagation();
        cx.notify();
      })
      .child(h_flex().items_center().child(div().flex_1().text_size(15).font_semibold().text_color(t.foreground).child("Sessions")).child(div().text_size(11).text_color(t.dim).child("Esc")))
      .child(
        h_flex().h(36).flex_none().px(10).gap(8).items_center().rounded(9).border_1().border_color(t.accent).bg(t.background)
          .child(svg("mods/workspace-chrome/search.svg").size(15).text_color(t.dim))
          .child(Input.new(this.input).flex_1().h_full().text_size(12.8).text_color(t.foreground)),
      )
      .when(Boolean(this.error), (el) => el.child(div().px(10).text_color(t.blocked).child(this.error)))
      .child(
        v_flex().min_h(0).flex_shrink(1).overflow_y_scrollbar().pb(6)
          .children(rows.map((row, index) =>
            h_flex()
              .id(`session-${index}`)
              .mx(4)
              .px(10)
              .h(34)
              .items_center()
              .rounded(7)
              .gap(10)
              .cursor_pointer()
              .when(row === selected, (el) => el.bg(t.hover))
              .hover((s) => s.bg(t.hover))
              .on_click(() => this.go(row))
              .child(div().w(12).text_color(row.space ? t.working : t.dim).child(row.space ? "●" : "○"))
              .child(div().flex_1().truncate().text_color(t.foreground).child(row.display))
              .child(div().text_size(11).text_color(row.recent ? t.accent : t.dim).child(row.recent ? "recent" : row.space ? "open" : "new")),
          ))
          .when(!rows.length && !this.error, (el) => el.child(div().px(10).py(12).text_color(t.dim).child("No matching session directories"))),
      );
  }
}
