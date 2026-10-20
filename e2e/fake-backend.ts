// Stands in for the Tauri backend: answers `window.__TAURI__.core.invoke` from fixture data,
// records every call, and emits `view://app` like the engine does after a change.

import type { Page } from "@playwright/test";
import type { Fixture } from "./fixtures";

export async function installFakeBackend(page: Page, fixture: Fixture) {
  await page.addInitScript((fixture) => {
    const state = structuredClone(fixture);
    const calls: Array<{ cmd: string; args: any }> = [];
    const listeners = new Map<string, Array<(message: any) => void>>();

    const emit = (event: string, payload: unknown) =>
      (listeners.get(event) ?? []).forEach((handler) => handler({ event, id: 0, payload }));
    const days = () => state.app.week?.days ?? [];
    const findEntry = (id: string) => {
      for (const day of days()) {
        const found = day.entries.find((entry: any) => entry.id === id);
        if (found) return found;
      }
      return null;
    };
    const minutes = (text: string) => {
      const value = text.trim();
      if (value.includes(":")) {
        const [hours, rest] = value.split(":");
        return Number(hours) * 60 + Number(rest);
      }
      return Math.round(parseFloat(value.replace(",", ".")) * 60);
    };
    const hm = (seconds: number) =>
      `${Math.floor(seconds / 3600)}:${String(Math.floor(seconds / 60) % 60).padStart(2, "0")}`;
    const changed = () => {
      for (const day of days()) {
        const sum = (kind: string) =>
          day.entries
            .filter((entry: any) => (entry.kind ?? "work") === kind)
            .reduce((total: number, entry: any) => total + entry.duration_seconds, 0);
        day.total_seconds = sum("work");
        day.break_seconds = sum("break");
      }
      if (state.app.week) {
        state.app.week.total_seconds = days().reduce((sum: number, day: any) => sum + day.total_seconds, 0);
      }
      emit("view://app", structuredClone(state.app));
    };

    const running = (port: number) => ({
      status: "running",
      url: `http://127.0.0.1:${port}/mcp`,
      bridge: "/Applications/Solidbar.app/Contents/MacOS/solidbar",
    });

    const handlers: Record<string, (args: any) => unknown> = {
      bootstrap: () => ({ app: structuredClone(state.app), catalog: structuredClone(state.catalog) }),
      select_date: ({ date }) => {
        state.app.selected_date = date;
        changed();
      },
      prepare_form: ({ date, entry_id, group_ids = [] }) => {
        const group = group_ids.map(findEntry);
        const existing = entry_id ? findEntry(entry_id) : (group[0] ?? null);
        const seconds = group.length
          ? group.reduce((sum: number, entry: any) => sum + entry.duration_seconds, 0)
          : existing?.duration_seconds;
        return {
          entry_id: entry_id ?? null,
          group_ids,
          date,
          // A new entry starts where the day's last one ends, or at 09:00.
          start_time: existing
            ? "09:00"
            : (days()
                .find((day: any) => day.date === date)
                ?.entries.filter((entry: any) => entry.end_label)
                .at(-1)?.end_label ?? "09:00"),
          duration: existing ? hm(seconds) : "",
          project_id: existing?.project_id ?? null,
          task_id: existing?.task_id ?? null,
          description: existing?.description ?? "",
          billable: existing?.billable ?? false,
          running: existing?.running ?? false,
          kind: existing?.kind ?? "work",
        };
      },
      save_entry: (draft) => {
        const seconds = minutes(draft.duration || "0") * 60;
        const option = state.catalog.options.find(
          (option: any) => option.project_id === draft.project_id && option.task_id === draft.task_id,
        );
        if (draft.id) {
          Object.assign(findEntry(draft.id), { description: draft.description, duration_seconds: seconds });
        } else {
          const [project, task] = (option?.label ?? "").split(" › ");
          days()
            .find((day: any) => day.date === draft.date)
            .entries.push({
              id: `new-${calls.length}`,
              kind: draft.kind ?? "work",
              project_id: draft.project_id,
              task_id: draft.task_id,
              project_name: option?.project_id ? project : null,
              task_name: task ?? null,
              client_name: option?.client_name ?? null,
              color: option?.color ?? null,
              description: draft.description.trim(),
              billable: draft.billable,
              start_label: draft.start_time,
              end_label: draft.start_timer ? null : "13:00",
              duration_seconds: seconds,
              running: draft.start_timer,
              pending: false,
            });
        }
        changed();
        emit("view://notice", { kind: "info", text: `Added ${seconds / 3600} hours` });
        return null;
      },
      adjust_entry: ({ id, delta_minutes }) => {
        findEntry(id).duration_seconds += delta_minutes * 60;
        changed();
        return null;
      },
      delete_entries: ({ ids }) => {
        for (const day of days()) day.entries = day.entries.filter((entry: any) => !ids.includes(entry.id));
        changed();
        return null;
      },
      select_org: ({ membership_id }) => {
        state.app.active_membership_id = membership_id;
        state.app.choose_org = false;
        changed();
      },
      update_settings: (patch) => {
        for (const [key, value] of Object.entries(patch)) {
          if (value !== null) state.app.settings[key] = value;
        }
        changed();
      },
      get_autostart: () => false,
      set_agent_access: ({ enabled }) => {
        const agents = state.app.settings.agents;
        agents.enabled = enabled;
        agents.port ??= 52731;
        agents.key ??= "k3y-1";
        state.app.agents = enabled ? running(agents.port) : { status: "off" };
        changed();
      },
      reset_agent_key: () => {
        state.app.settings.agents.key = "k3y-2";
        changed();
      },
      move_agent_server: () => {
        state.app.settings.agents.port = 52800;
        state.app.agents = running(52800);
        changed();
      },
      update_get: () => structuredClone(update),
      update_check: () => {
        update.state = state.updateCheck ?? { state: "up_to_date" };
        emit("update://changed", structuredClone(update));
        return structuredClone(update);
      },
      update_install: () => {
        update.state = { state: "installing", version: update.state.version, percent: 40 };
        emit("update://changed", structuredClone(update));
        return null;
      },
    };
    const update = state.update ?? { current_version: "0.1.0", state: { state: "idle" } };

    (window as any).__calls = calls;
    (window as any).__emit = emit;
    // Like the panel: shown, it goes back to today (`Engine::panel_opened`).
    (window as any).__showPanel = (reason: "toggle" | "new_entry") => {
      emit("panel://shown", reason);
      state.app.selected_date = state.app.today;
      changed();
    };
    (window as any).__hidePanel = () => emit("panel://hidden", null);
    // Like the panel dragged away from the menu bar (true) and closed again (false).
    (window as any).__detachPanel = (detached: boolean) => emit("panel://detached", detached);
    (window as any).__TAURI__ = {
      core: {
        invoke: async (cmd: string, payload: any) => {
          const args = payload?.args ?? null;
          calls.push({ cmd, args });
          const handler = handlers[cmd];
          return handler ? (handler(args ?? {}) ?? null) : null;
        },
      },
      event: {
        listen: async (event: string, handler: (message: any) => void) => {
          if (!listeners.has(event)) listeners.set(event, []);
          listeners.get(event)!.push(handler);
          return () => {};
        },
      },
    };
  }, fixture);

  // Headless pages are white; the real panel sits on the macOS popover material.
  await page.addInitScript(() => {
    const style = document.createElement("style");
    style.textContent = `
      html { background: #ececec; }
      @media (prefers-color-scheme: dark) { html { background: #2a2a2c; } }`;
    document.addEventListener("DOMContentLoaded", () => document.head.append(style));
  });
}

export async function invoked(page: Page, cmd: string) {
  return page.evaluate(
    (cmd) => (window as any).__calls.filter((call: any) => call.cmd === cmd).map((call: any) => call.args),
    cmd,
  );
}
