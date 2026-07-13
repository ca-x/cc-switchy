# TUI Interaction Repair Design

**Date:** 2026-07-13
**Status:** Approved for implementation planning

## 1. Problem

The source wizard maps keys without considering its current mode. Printable
characters such as `q`, `j`, `k`, `a`, `e`, `x`, `t`, `m`, `w`, `s`, and `L`
become commands before an edit form can receive them. A user entering a source
name, URL, username, or secret can therefore leave the form or silently lose
characters.

The terminal cursor is hidden for the entire TUI session. The active form row
only styles the field value, so an empty active field has no visible focus.
Responsive layouts have two related problems: the Providers view can focus a
hidden Details pane at medium widths, and the Skills view can keep focus on a
hidden Agents pane at narrow widths. Static footers also advertise actions
that do nothing in the current mode.

The current render tests pass because they call `WizardState::update` with
pre-built actions. They do not exercise the key-to-action boundary or assert
that an empty field and focused pane are visible.

## 2. Scope

This change will:

- make wizard key handling depend on `WizardMode`;
- treat printable characters as input while a form is active;
- give the active field a text marker, full-row style, and terminal cursor;
- make `q`, `Esc`, and `Ctrl+C` consistent across wizard modes;
- show footer hints for the current wizard mode and main TUI view;
- keep form values visible when validation or persistence fails;
- prevent responsive layouts from focusing a pane that is not rendered;
- add regression tests at the key mapping, state, and render boundaries;
- update the keyboard documentation in both supported languages.

This change will not add arbitrary cursor movement within a field, mouse
support, clipboard handling, or an asynchronous connection-test state machine.
Those require a separate form editor or runtime task design.

## 3. Wizard Input Contract

The key mapper will receive the current wizard state or mode. It will apply the
following priority rules.

### EditWebDav and EditS3

1. `Ctrl+C` queues `WizardCommand::Exit`.
2. `Esc` discards the form and returns to the source list.
3. `Tab` and `Shift+Tab` move between fields.
4. `Enter` advances to the next field or submits the last field.
5. `Backspace` removes the last character.
6. Every printable `KeyCode::Char` appends that character to the active field.

No single-character command shortcut is active in a form. This includes `q`,
navigation letters, source-type shortcuts, and language shortcuts.

### List

The existing list shortcuts remain available. `q` and `Ctrl+C` exit the
wizard. `Esc` has no destructive effect.

### Details, type selection, confirmation, replacement, testing, and language

`Esc` returns to the previous safe screen without applying a mutation. `q` and
`Ctrl+C` exit the wizard. Mode-specific navigation and confirmation keys remain
available.

The same rules apply when the wizard runs standalone or inside the main TUI.
Exiting an embedded wizard returns to the main TUI because the command runner
owns that boundary.

## 4. Focus Rendering

Each form row will reserve a marker column. The active row will show `>` or
`›`, use the accent style across the row, and place the terminal cursor at the
end of the rendered value. Empty fields will therefore remain visible without
relying on color.

Secret fields will continue to render masked characters. The cursor position
will follow the displayed mask rather than the secret's raw bytes. Rendering
must calculate display width by terminal cells so non-ASCII text does not move
the cursor to the wrong column.

When no form is active, the renderer will not set a cursor position. Ratatui
will hide the cursor again on the next draw.

Main TUI pane titles will retain their accent border and gain a text-level
focus signal where needed. Responsive rendering will follow these rules:

- Providers at wide widths renders Agents, Providers, and Details.
- Providers at medium widths renders Agents plus the focused Providers or
  Details pane.
- Providers at narrow widths renders only the focused pane.
- Skills at wide and medium widths renders Agents and Skills.
- Skills at narrow widths renders the focused Agents or Skills pane.
- Sources continues to render both panes when space permits and the focused
  pane otherwise.

Every focus transition must produce a visible pane change or visible focus
style.

## 5. Contextual Help

Wizard footer text will be selected by mode. List hints will not appear below
forms or confirmation dialogs. Form hints will state the input controls,
including `Esc` and `Ctrl+C`. Confirmation and selection screens will show
their navigation, confirm, back, and exit controls.

The main TUI footer will be selected by view. Providers will describe apply
and Agent navigation, Sources will describe sync, test, default, and wizard
actions, Activity will describe retry only when it is available, and Skills
will avoid advertising provider actions.

Chinese and English strings must describe the same behavior.

## 6. Submission and Error Handling

Submitting the last field creates a candidate source but does not clear the
form immediately. The wizard keeps the active mode, fields, and original source
name until the command runner reports success.

On success, the wizard returns to the list, clears edit-only state, refreshes
the source catalog, and displays the saved status. On validation or persistence
failure, it stays on the same form, keeps every field value, and displays the
error. A repeated Enter can retry after correction.

The design should reuse catalog validation instead of maintaining a second set
of URL, uniqueness, and required-field rules in the TUI.

## 7. Tests and Verification

Automated tests will cover:

- each command-like printable character entering the active form unchanged;
- `q`, `Esc`, and `Ctrl+C` behavior in forms and non-form modes;
- form values surviving a failed add or update;
- an empty active field rendering a marker and cursor position;
- secret fields keeping their raw value out of the rendered buffer;
- contextual Chinese and English footer text;
- Providers focus visibility at wide, medium, and narrow widths;
- Skills focus visibility at wide and narrow widths;
- the existing source CRUD, masking, navigation, and persisted UI tests.

Verification will run the focused TUI test target, the full test suite, format,
and Clippy. A PTY smoke test will open the wizard, enter `qjaketmxwsL` into an
empty field, move across fields, cancel once, submit once, and confirm that the
terminal cursor is restored after exit.

## 8. Audited Sibling Risks

The source-connection test in the standalone wizard awaits network work inside
the event loop. Its testing message may not render before the request begins,
and the user cannot cancel it. The main TUI already runs source tests on a
Tokio task. This change records that difference but leaves it for a separate
runtime-lifecycle fix because it is independent of input dispatch and focus.

Provider application can also be triggered while focus is on Agents or
Details. The current product contract treats Enter as a view-level action, so
this repair will keep that behavior rather than add a confirmation step.
