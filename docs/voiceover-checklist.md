# VoiceOver checklist

Automated checks (axe in `e2e/`) catch missing labels and contrast problems. They can't tell whether the panel is pleasant to use with a screen reader, so run through this before each release with VoiceOver on (`⌘F5`).

Use a build installed in `/Applications`, with System Settings › Accessibility › Display › "Increase contrast" and "Reduce motion" off for the first pass and on for a second pass.

## Menu bar

- [ ] VO reads the menu bar item as "Solidbar" with "Today: 6:30" (or "Timer running: Project, 0:42", or "On a break, 0:05") as its description.
- [ ] `⌥⌘T` opens the panel and VO lands on today's first entry (or on today's tab when the day is empty).
- [ ] `⎋` closes the panel and VO is back in the app you came from.

## Week strip and list

- [ ] Tabbing reaches, in order: previous week, next week, Today, Keep open, Settings, the selected day tab, the selected entry, New entry.
- [ ] Day tabs read as "Friday, Oct 9, 2 hours, today, selected, tab, 5 of 7".
- [ ] `←`/`→` on a day tab move to the neighbouring day and VO announces it.
- [ ] Entries read as one sentence: project › task, client, notes, duration, start and end, billable.
- [ ] `↑`/`↓` move between entries; VO reads each one.
- [ ] VO announces the keyboard shortcuts of an entry (aria-keyshortcuts) with the hint delay.
- [ ] With Settings › Single-key shortcuts off, N, T, S, B, E and R do nothing in the list, and VO no longer announces them (⌘N, ↩ and the arrows still work).
- [ ] `⌘=` announces "Added 1 hour" (and how many later entries moved); the duration updates.
- [ ] Holding `⌫` on an entry for 2 seconds deletes it and announces "Time entry deleted"; letting go earlier announces "Hold ⌫ to delete" and keeps it.
- [ ] The form's Delete reads its description ("Hold for 2 seconds, or hold Space, …"); holding Space deletes; `VO-Space` (which can't hold) asks "Delete this entry?" with Delete and Keep, focus on Delete.
- [ ] Rotor › Buttons › "Delete entry" on a row asks "Delete this entry?"; `⎋` returns to the entry; `↩` deletes.
- [ ] `S` announces "Timer started"; the row reads "timer running".
- [ ] A continued entry reads as one row with its total and "2 entries", and as "collapsed"; `→` reads it as "expanded" and `↓` reads each entry ("09:00 to 10:30, 1 hour 30 minutes"); `←` returns to the row, collapsed.
- [ ] `↩` on a continued row opens "Edit 2 entries" and reads that changes apply to all of them.
- [ ] With breaks on: `B` announces "Break started"; the "Break" region offers "Resume …"; the day total reads "… worked, … of breaks"; `S` announces "Timer started" and `B` (during a break) "Break ended".
- [ ] Going offline (turn off Wi-Fi, wait a minute) shows and reads the offline notice; the ±1h and timer buttons are dimmed and read as dimmed.

## Entry form

- [ ] `⌘N` opens the form; VO reads "New time entry · Friday, Oct 9" and lands on "Project and task, No project, pop-up button".
- [ ] `Space` opens the list on the current choice, in "Search projects and tasks, combo box"; `⎋` closes it and VO is back on the pop-up button.
- [ ] Typing on the pop-up button opens the search with that letter; VO reads the number of results; `↓`/`↑` read each option; `↩` picks it and returns to the button, which reads the new choice.
- [ ] `⌘1`/`⌘2` pick recent projects; VO reads the new value of the pop-up button.
- [ ] `⌥2` selects "2 hours" (button reads as selected) and fills the duration.
- [ ] An invalid duration reads as invalid with its error message.
- [ ] `↩` saves, VO announces "Added 2 hours", and focus returns to the list.
- [ ] `⎋` (with the list closed) cancels and focus returns to the entry it came from.

- [ ] "Start" reads as a text field with its time; `↑`/`↓` change it by 15 minutes and VO reads the new time; an invalid time reads as invalid with "Enter a time like 09:30".

## Settings and sign-in

- [ ] Signing in to several organizations lands on "Choose an organization" with the first one focused; `↑`/`↓` move, `↩` chooses and focus goes to today's first entry.
- [ ] `⌘O` opens Settings on "Organization for new entries".
- [ ] "Time format" reads as a radio group; switching it changes the times in the list and the form.

- [ ] Each shortcut reads as e.g. "Show or hide Solidbar ⌥⌘T, button"; after activating it, pressing a new combination saves it, `⎋` cancels.
- [ ] Theme options read as a radio group "Appearance".
- [ ] "Check for updates" reads the current version as its description; after activating it, VO announces "Solidbar is up to date" (or the new version) and focus stays on the button.
- [ ] "Licenses" reads the copyright and "without warranty" as its description; activating it opens the licenses page in the browser, where VO reaches each license by heading (VO-⌘H).
- [ ] "Let AI agents use Solidbar" is a switch with its explanation as description; turning it on reveals "Address" and "Key" fields, each with a "Copy the …" button, and VO announces "Copied the address" (or the key, or the setup) after copying.
- [ ] "New key" keeps focus on the button and the Key field gets the new key; a taken port is announced as an alert with a "Use another port" button.
- [ ] The sign-in form's fields have labels, the redirect URIs are read with the client ID field, "Checking the OAuth client…" and then "Waiting for you to approve…" are announced, and errors are announced right away.

## Visual

- [ ] Light, dark, "Increase contrast" and "Reduce transparency": all text is readable, focus rings are visible.
- [ ] "Reduce motion": no pulsing dot on a running entry, no toast animation.
- [ ] Larger text (System Settings › Accessibility › Display › Text size, where supported) doesn't clip entry rows.
