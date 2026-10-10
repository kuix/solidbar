// A believable week for the README screenshots.

import { entry, type Fixture, signedIn } from "../fixtures";

const design = { project_id: "p1", project_name: "Design system", client_name: "Kuix", color: "#ef5350" };
const support = { project_id: "p3", task_id: null, project_name: "Support", task_name: null, client_name: "ACME", color: "#66bb6a" };
const solidbar = { project_id: "p4", project_name: "Solidbar", client_name: null, color: "#7c6cf2" };

export function showcase(): Fixture {
  const fixture = signedIn();
  const week = fixture.app.week;
  const past: Record<string, [number, Record<string, unknown>]> = {
    "2026-10-05": [8 * 3600, { ...design, task_id: "t1", task_name: "Header" }],
    "2026-10-06": [7.5 * 3600, { ...support }],
    "2026-10-07": [8.25 * 3600, { ...design, task_id: "t2", task_name: "Footer" }],
    "2026-10-08": [6.75 * 3600, { ...solidbar, task_id: "t5", task_name: "Release" }],
  };
  for (const day of week.days) {
    if (past[day.date]) {
      const [seconds, work] = past[day.date];
      day.entries = [entry({ id: `past-${day.date}`, ...work, duration_seconds: seconds })];
    }
  }
  const today = week.days.find((day: any) => day.date === "2026-10-09");
  today.entries = [
    entry({ id: "e1", ...design, task_id: "t1", task_name: "Header", description: "Header components", start_label: "09:00", end_label: "10:30", duration_seconds: 5400 }),
    entry({ id: "e2", ...support, description: "Ticket #123: CSV export", start_label: "10:30", end_label: "12:00", duration_seconds: 5400 }),
    entry({ id: "e3", ...solidbar, task_id: "t4", task_name: "README", description: "Screenshots and install guide", start_label: "13:00", end_label: null, duration_seconds: 7920, running: true, billable: false }),
  ];
  for (const day of week.days) {
    day.total_seconds = day.entries.reduce((sum: number, item: any) => sum + item.duration_seconds, 0);
  }
  week.total_seconds = week.days.reduce((sum: number, day: any) => sum + day.total_seconds, 0);

  const option = (project_id: string | null, task_id: string | null, label: string, client_name: string | null, color: string | null) => ({
    project_id, task_id, label, client_name, color, billable: client_name !== null,
  });
  fixture.catalog = {
    options: [
      option("p1", null, "Design system", "Kuix", "#ef5350"),
      option("p1", "t2", "Design system › Footer", "Kuix", "#ef5350"),
      option("p1", "t1", "Design system › Header", "Kuix", "#ef5350"),
      option("p5", "t6", "Internal › Planning", null, "#ffa726"),
      option("p4", null, "Solidbar", null, "#7c6cf2"),
      option("p4", "t4", "Solidbar › README", null, "#7c6cf2"),
      option("p4", "t5", "Solidbar › Release", null, "#7c6cf2"),
      option("p3", null, "Support", "ACME", "#66bb6a"),
      option(null, null, "No project", null, null),
    ],
    recents: [
      option("p4", "t4", "Solidbar › README", null, "#7c6cf2"),
      option("p1", "t1", "Design system › Header", "Kuix", "#ef5350"),
      option("p3", null, "Support", "ACME", "#66bb6a"),
    ],
  };
  fixture.app.timer = {
    entry_id: "e3",
    membership_id: "m1",
    org_name: "Kuix",
    started_at: "2026-10-09T11:00:00Z",
    description: "Screenshots and install guide",
    project_name: "Solidbar",
    in_active_org: true,
  };
  return fixture;
}
