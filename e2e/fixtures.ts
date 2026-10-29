// View models in the shape the backend sends (see crates/shared/src/view.rs).

export type Fixture = { app: any; catalog: any; update?: any; updateCheck?: any };

const days = [
  ["2026-10-05", "Mon", "Monday, Oct 5"],
  ["2026-10-06", "Tue", "Tuesday, Oct 6"],
  ["2026-10-07", "Wed", "Wednesday, Oct 7"],
  ["2026-10-08", "Thu", "Thursday, Oct 8"],
  ["2026-10-09", "Fri", "Friday, Oct 9"],
  ["2026-10-10", "Sat", "Saturday, Oct 10"],
  ["2026-10-11", "Sun", "Sunday, Oct 11"],
] as const;

export function entry(overrides: Record<string, unknown>) {
  return {
    id: "e1",
    kind: "work",
    project_id: "p1",
    task_id: "t1",
    project_name: "Design system",
    task_name: "Header",
    client_name: "Kuix",
    color: "#ef5350",
    description: "Header components",
    billable: true,
    start_label: "09:00",
    end_label: "10:30",
    duration_seconds: 5400,
    running: false,
    pending: false,
    ...overrides,
  };
}

const settings = {
  instance: {
    url: "https://time.example.com",
    client_id: "client-1",
  },
  active_membership_id: "m1",
  workday_start: "09:00",
  theme: "system",
  clock: "system",
  menu_bar_text: true,
  shortcuts: {
    toggle_panel: "Alt+Super+KeyT",
    new_entry: "Alt+Super+KeyN",
    toggle_timer: "Alt+Super+KeyS",
  },
  single_key_shortcuts: true,
  launch_at_login_offered: true,
  agents: { enabled: false, port: null, key: null },
};

const catalog = {
  options: [
    { project_id: "p2", task_id: null, label: "Admin", client_name: null, color: "#42a5f5", billable: false },
    { project_id: "p1", task_id: null, label: "Design system", client_name: "Kuix", color: "#ef5350", billable: true },
    { project_id: "p1", task_id: "t1", label: "Design system › Header", task_name: "Header", client_name: "Kuix", color: "#ef5350", billable: true },
    { project_id: "p3", task_id: null, label: "Support", client_name: "ACME", color: "#66bb6a", billable: true },
    { project_id: null, task_id: null, label: "No project", client_name: null, color: null, billable: false },
  ],
  recents: [
    { project_id: "p1", task_id: "t1", label: "Design system › Header", task_name: "Header", client_name: "Kuix", color: "#ef5350", billable: true },
    { project_id: "p3", task_id: null, label: "Support", client_name: "ACME", color: "#66bb6a", billable: true },
  ],
  favorites: [],
};

export function signedIn(): Fixture {
  const entries: Record<string, any[]> = {
    "2026-10-05": [entry({ id: "monday", duration_seconds: 8 * 3600, end_label: "17:00", description: "" })],
    "2026-10-09": [
      entry({ id: "e1" }),
      entry({
        id: "e2",
        project_id: "p3",
        task_id: null,
        project_name: "Support",
        task_name: null,
        client_name: "ACME",
        color: "#66bb6a",
        description: "Ticket #123",
        start_label: "10:30",
        end_label: "11:00",
        duration_seconds: 1800,
      }),
    ],
  };
  const weekDays = days.map(([date, weekday_short, label]) => {
    const list = entries[date] ?? [];
    return {
      date,
      weekday_short,
      label,
      is_today: date === "2026-10-09",
      total_seconds: list.reduce((sum, item) => sum + item.duration_seconds, 0),
      break_seconds: 0,
      entries: list,
    };
  });
  return {
    app: {
      auth: { status: "signed_in", user_name: "Barnabás", instance_url: settings.instance.url },
      // Copies, so a test that changes them doesn't change them for the next one.
      settings: structuredClone(settings),
      orgs: [
        { membership_id: "m2", name: "ACME" },
        { membership_id: "m1", name: "Kuix" },
      ],
      active_membership_id: "m1",
      choose_org: false,
      hour12: false,
      today: "2026-10-09",
      selected_date: "2026-10-09",
      week: {
        first_day: "2026-10-05",
        label: "Oct 5 – 11, 2026",
        days: weekDays,
        total_seconds: weekDays.reduce((sum, day) => sum + day.total_seconds, 0),
      },
      timer: null,
      online: true,
      loading: false,
      time_zone_hint: null,
      tray: { timer_running: false, title: "2:00", tooltip: "Today: 2:00" },
      breaks_enabled: false,
      agents: { status: "off" },
    },
    catalog: structuredClone(catalog),
  };
}

/** A break, as solidtime sends it: no project, task or billable flag. */
export function pause(overrides: Record<string, unknown>) {
  return entry({
    kind: "break",
    project_id: null,
    task_id: null,
    project_name: null,
    task_name: null,
    client_name: null,
    color: null,
    description: "",
    billable: false,
    ...overrides,
  });
}

/** Replaces Friday's entries and recounts the totals as the backend would. */
export function withFriday(fixture: Fixture, entries: any[]): Fixture {
  const friday = fixture.app.week.days.find((day: any) => day.date === "2026-10-09");
  friday.entries = entries;
  for (const day of fixture.app.week.days) {
    const sum = (kind: string) =>
      day.entries
        .filter((item: any) => item.kind === kind)
        .reduce((total: number, item: any) => total + item.duration_seconds, 0);
    day.total_seconds = sum("work");
    day.break_seconds = sum("break");
  }
  fixture.app.week.total_seconds = fixture.app.week.days.reduce(
    (sum: number, day: any) => sum + day.total_seconds,
    0,
  );
  return fixture;
}

/** The organization tracks breaks: "Break" is in the picker and B takes one. */
export function withBreaks(fixture: Fixture): Fixture {
  fixture.app.breaks_enabled = true;
  fixture.catalog.options.push({
    kind: "break",
    project_id: null,
    task_id: null,
    label: "Break",
    client_name: null,
    color: null,
    billable: false,
  });
  return fixture;
}

/** Friday's header work was continued after a call: two entries, one row. */
export function continued(): Fixture {
  return withFriday(signedIn(), [
    entry({ id: "e1" }),
    entry({
      id: "e2",
      project_id: "p3",
      task_id: null,
      project_name: "Support",
      task_name: null,
      client_name: "ACME",
      color: "#66bb6a",
      description: "Ticket #123",
      start_label: "10:30",
      end_label: "11:00",
      duration_seconds: 1800,
    }),
    entry({ id: "e3", start_label: "11:00", end_label: "11:20", duration_seconds: 1200 }),
  ]);
}

/** On a break after the header work, which "Resume" goes back to. */
export function onBreak(): Fixture {
  const fixture = withBreaks(
    withFriday(signedIn(), [
      entry({ id: "e1" }),
      pause({ id: "pause", start_label: "10:30", end_label: null, duration_seconds: 900, running: true }),
    ]),
  );
  fixture.app.timer = {
    entry_id: "pause",
    membership_id: "m1",
    org_name: "Kuix",
    started_at: "2026-10-09T08:30:00Z",
    description: "",
    project_name: null,
    in_active_org: true,
    kind: "break",
    resume_label: "Header components",
  };
  fixture.app.tray = { timer_running: true, title: "☕ 0:15", tooltip: "On a break, 0:15" };
  return fixture;
}

/** Signed in to two organizations without having picked one. */
export function choosingOrg(): Fixture {
  const fixture = signedIn();
  fixture.app.active_membership_id = null;
  fixture.app.choose_org = true;
  return fixture;
}

export function signedOut(error: string | null = null): Fixture {
  return {
    app: {
      ...signedIn().app,
      auth: { status: "signed_out", instance: settings.instance, error },
      orgs: [],
      active_membership_id: null,
      week: null,
      tray: { timer_running: false, title: null, tooltip: "Solidbar — not signed in" },
    },
    catalog: { options: [], recents: [] },
  };
}

/** Signing in: `redirect` is null while Solidbar checks which redirect the client allows. */
export function signingIn(redirect: { mode: "loopback"; port: number } | { mode: "deep_link" } | null): Fixture {
  const fixture = signedOut();
  fixture.app.auth = { status: "signing_in", instance: settings.instance, redirect };
  return fixture;
}
