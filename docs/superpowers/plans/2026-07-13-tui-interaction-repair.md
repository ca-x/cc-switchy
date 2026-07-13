# TUI Interaction Repair Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Make source forms accept normal text input, keep focus visible in every responsive layout, preserve failed submissions, and release the repair as `v0.1.1`.

**Architecture:** Keep the existing pure TUI state model and make the key boundary mode-aware. Ratatui rendering will own cursor placement and contextual help, while the command runner will report catalog mutation success or failure back to `WizardState` before the form is cleared.

**Tech Stack:** Rust 1.95, Crossterm 0.29, Ratatui 0.30, Tokio 1, Cargo integration tests, Git tags, GitHub remote over SSH.

## Global Constraints

- Printable characters must reach form fields unchanged, including `q`, `j`, `k`, `a`, `e`, `x`, `t`, `m`, `w`, `s`, and `L`.
- `Esc` discards an active form or returns one screen; `q` exits only outside forms; `Ctrl+C` exits from every wizard mode.
- Focus must remain visible without depending on color.
- Secret values must never appear in rendered buffers, logs, test failure messages, or release output.
- Chinese and English help text must describe the same controls.
- Catalog validation remains the single authority for URL, required-field, uniqueness, and persistence errors.
- Do not add field-internal cursor movement, mouse support, clipboard support, or an asynchronous connection-test state machine.
- Release only after focused tests, the full suite, formatting, Clippy, a release build, and a PTY smoke test pass.

---

## File Map

- `src/tui/wizard.rs`: wizard mode transitions, key mapping, form state, field rendering, and cursor placement.
- `src/commands/wizard.rs`: catalog mutation execution and success/failure feedback to the form state.
- `src/tui/view.rs`: responsive pane rendering, text-level focus markers, and view-specific footer hints.
- `src/i18n.rs`: Chinese and English contextual footer strings.
- `tests/tui_render.rs`: key-boundary, state-lifecycle, cursor, masking, footer, and responsive-focus regression tests.
- `README.md`: public keyboard contract and release-specific security limit.
- `Cargo.toml`, `Cargo.lock`: package version `0.1.1`.

### Task 1: Make wizard input dispatch mode-aware

**Files:**
- Modify: `src/tui/wizard.rs:1-430`
- Modify: `src/commands/wizard.rs:45-55`
- Test: `tests/tui_render.rs`

**Interfaces:**
- Consumes: `WizardState::mode`, `KeyEvent`, `WizardAction`, and `WizardCommand`.
- Produces: `pub fn action_for_key(state: &WizardState, key: KeyEvent) -> Option<WizardAction>` and a global `WizardAction::Quit` transition that queues `WizardCommand::Exit`.

- [ ] **Step 1: Add failing key-boundary tests**

Add the wizard module import and tests that exercise actual `KeyEvent` values:

```rust
use cc_switchy::tui::wizard;

#[test]
fn wizard_form_treats_command_letters_as_text() {
    let mut state = WizardState::new(Language::EnUs, Vec::new(), None);
    state.update(WizardAction::Add);
    state.update(WizardAction::Confirm);

    for character in "qjkaextmw sL".replace(' ', "").chars() {
        let action = wizard::action_for_key(&state, key(KeyCode::Char(character)))
            .expect("form input action");
        state.update(action);
    }

    assert_eq!(state.form_values()[0], "qjkaextmwsL");
    assert_eq!(state.mode, cc_switchy::tui::WizardMode::EditWebDav);
    assert_eq!(state.pop_command(), None);
}

#[test]
fn wizard_exit_and_back_keys_follow_mode() {
    let mut state = WizardState::new(Language::EnUs, Vec::new(), None);
    state.update(WizardAction::Add);
    state.update(WizardAction::Confirm);

    let q = wizard::action_for_key(&state, key(KeyCode::Char('q'))).expect("q input");
    state.update(q);
    assert_eq!(state.form_values()[0], "q");

    let escape = wizard::action_for_key(&state, key(KeyCode::Esc)).expect("escape");
    state.update(escape);
    assert_eq!(state.mode, cc_switchy::tui::WizardMode::List);

    state.update(WizardAction::Add);
    let quit = wizard::action_for_key(&state, key(KeyCode::Char('q'))).expect("quit");
    state.update(quit);
    assert!(matches!(state.pop_command(), Some(WizardCommand::Exit)));
}

#[test]
fn control_c_exits_from_a_wizard_form() {
    let mut state = WizardState::new(Language::EnUs, Vec::new(), None);
    state.update(WizardAction::Add);
    state.update(WizardAction::Confirm);
    let control_c = KeyEvent::new(KeyCode::Char('c'), KeyModifiers::CONTROL);

    let action = wizard::action_for_key(&state, control_c).expect("Ctrl+C");
    state.update(action);

    assert!(matches!(state.pop_command(), Some(WizardCommand::Exit)));
}
```

- [ ] **Step 2: Run the new tests and verify the old mapper fails**

Run:

```bash
cargo test --test tui_render wizard_ -- --nocapture
```

Expected: compilation fails because `action_for_key` does not accept `&WizardState`, or the assertions fail because `q` and other letters become commands.

- [ ] **Step 3: Implement mode-aware key priority**

Change the mapper signature and handle form controls before non-form shortcuts:

```rust
use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};

pub fn action_for_key(state: &WizardState, key: KeyEvent) -> Option<WizardAction> {
    if key.modifiers.contains(KeyModifiers::CONTROL) && key.code == KeyCode::Char('c') {
        return Some(WizardAction::Quit);
    }

    if matches!(state.mode, WizardMode::EditWebDav | WizardMode::EditS3) {
        return match key.code {
            KeyCode::Esc => Some(WizardAction::Cancel),
            KeyCode::Enter => Some(WizardAction::Confirm),
            KeyCode::Tab => Some(WizardAction::NextField),
            KeyCode::BackTab => Some(WizardAction::PreviousField),
            KeyCode::Backspace => Some(WizardAction::Backspace),
            KeyCode::Char(character) => Some(WizardAction::Input(character)),
            _ => None,
        };
    }

    match key.code {
        KeyCode::Char('q') => Some(WizardAction::Quit),
        KeyCode::Esc => Some(WizardAction::Cancel),
        KeyCode::Up | KeyCode::Char('k') => Some(WizardAction::Move(-1)),
        KeyCode::Down | KeyCode::Char('j') => Some(WizardAction::Move(1)),
        KeyCode::Enter => Some(WizardAction::Confirm),
        KeyCode::Char('a') => Some(WizardAction::Add),
        KeyCode::Char('e') => Some(WizardAction::Edit),
        KeyCode::Char('x') => Some(WizardAction::Delete),
        KeyCode::Char('t') => Some(WizardAction::Test),
        KeyCode::Char('m') => Some(WizardAction::MakeDefault),
        KeyCode::Char('L') => Some(WizardAction::Language),
        KeyCode::Char('w') => Some(WizardAction::ChooseWebDav),
        KeyCode::Char('s') => Some(WizardAction::ChooseS3),
        _ => None,
    }
}
```

Make quit global before the mode-specific transition:

```rust
pub fn update(&mut self, action: WizardAction) {
    if action == WizardAction::Quit {
        self.commands.push_back(WizardCommand::Exit);
        return;
    }
    match self.mode {
        WizardMode::List => self.update_list(action),
        WizardMode::Details | WizardMode::TestConnection => match action {
            WizardAction::Cancel | WizardAction::Confirm => self.mode = WizardMode::List,
            _ => {}
        },
        WizardMode::ChooseType => self.update_choose_type(action),
        WizardMode::EditWebDav | WizardMode::EditS3 => self.update_form(action),
        WizardMode::ConfirmDelete => self.update_delete(action),
        WizardMode::ChooseReplacementDefault => self.update_replacement(action),
        WizardMode::LanguageSelect => self.update_language(action),
    }
}
```

In `update_choose_type`, `update_form`, `update_delete`, `update_replacement`, and `update_language`, keep `WizardAction::Cancel` as the back/discard arm and remove `WizardAction::Quit` from those arms. Remove the list-local `WizardAction::Quit` branch because the global branch now owns exit.

Update the event loop call site:

```rust
if let Some(action) = wizard::action_for_key(&state, key) {
    state.update(action);
    dirty = true;
}
```

- [ ] **Step 4: Run focused wizard input tests**

Run: `cargo test --test tui_render wizard_ -- --nocapture`

Expected: all wizard input, CRUD, masking, and localization tests pass.

- [ ] **Step 5: Commit the input fix**

```bash
git add src/tui/wizard.rs src/commands/wizard.rs tests/tui_render.rs
git commit -m "Fix text entry in source forms"
```

### Task 2: Preserve form data until catalog mutation succeeds

**Files:**
- Modify: `src/tui/wizard.rs:210-255`
- Modify: `src/commands/wizard.rs:55-115`
- Test: `tests/tui_render.rs`

**Interfaces:**
- Consumes: `WizardCommand::Add`, `WizardCommand::Update`, `SourceCatalog::add`, and `SourceCatalog::update`.
- Produces: `WizardState::mutation_succeeded(Vec<SourceConfig>, Option<String>)` and `WizardState::mutation_failed(String)`.

- [ ] **Step 1: Add failing form-lifecycle tests**

```rust
#[test]
fn wizard_keeps_form_values_when_catalog_mutation_fails() {
    let mut state = WizardState::new(Language::EnUs, Vec::new(), None);
    state.update(WizardAction::Add);
    state.update(WizardAction::Confirm);
    for character in "duplicate".chars() {
        state.update(WizardAction::Input(character));
    }
    state.update(WizardAction::NextField);
    for character in "https://dav.example.test".chars() {
        state.update(WizardAction::Input(character));
    }
    state.update(WizardAction::NextField);
    for character in "user".chars() {
        state.update(WizardAction::Input(character));
    }
    state.update(WizardAction::NextField);
    for character in "secret".chars() {
        state.update(WizardAction::Input(character));
    }
    state.update(WizardAction::NextField);
    state.update(WizardAction::NextField);
    let before = state.form_values();
    state.update(WizardAction::Confirm);
    assert!(matches!(state.pop_command(), Some(WizardCommand::Add(_))));

    state.mutation_failed("source already exists".to_string());

    assert_eq!(state.mode, cc_switchy::tui::WizardMode::EditWebDav);
    assert_eq!(state.form_values(), before);
    assert!(state.status.as_deref().unwrap_or_default().contains("already exists"));
}

#[test]
fn wizard_clears_form_only_after_catalog_mutation_succeeds() {
    let source = sample_webdav_source("home");
    let mut state = WizardState::new(Language::EnUs, Vec::new(), None);
    state.update(WizardAction::Add);
    state.update(WizardAction::Confirm);

    state.mutation_succeeded(vec![source], Some("home".to_string()));

    assert_eq!(state.mode, cc_switchy::tui::WizardMode::List);
    assert!(state.form_values().is_empty());
    assert_eq!(state.status.as_deref(), Some("✓ Saved"));
}
```

Use a local `sample_webdav_source` helper with non-secret fixture values already used by the test module.

- [ ] **Step 2: Run the lifecycle tests and verify missing methods fail**

Run:

```bash
cargo test --test tui_render wizard_ -- --nocapture
```

Expected: compilation fails because the mutation completion methods do not exist.

- [ ] **Step 3: Keep edit state while queuing add or update**

In the final-field submit branch, clone `edit_original` and leave the mode and fields unchanged:

```rust
WizardAction::Confirm => {
    if let Some(source) = self.build_source() {
        if let Some(original) = self.edit_original.clone() {
            self.commands
                .push_back(WizardCommand::Update { original, source });
        } else {
            self.commands.push_back(WizardCommand::Add(source));
        }
    } else {
        self.status = Some(
            Translator::new(self.language)
                .text(MessageKey::WizardRequired, &MessageArgs::default()),
        );
    }
}
```

Add completion methods:

```rust
pub fn mutation_succeeded(
    &mut self,
    sources: Vec<SourceConfig>,
    default_source: Option<String>,
) {
    self.update_sources(sources, default_source);
    self.fields.clear();
    self.edit_original = None;
    self.mode = WizardMode::List;
    self.status = Some(
        Translator::new(self.language).text(MessageKey::WizardSaved, &MessageArgs::default()),
    );
}

pub fn mutation_failed(&mut self, error: String) {
    self.status = Some(format!("× {error}"));
}
```

- [ ] **Step 4: Return catalog state from the command runner**

Replace the state-mutating helper with a result-returning helper:

```rust
fn mutate_catalog(
    paths: &AppPaths,
    mutation: impl FnOnce(&mut SourceCatalog) -> Result<(), AppError>,
) -> Result<(Vec<SourceConfig>, Option<String>), AppError> {
    let mut catalog = SourceCatalog::load(ConfigStore::new(paths.config_file.clone()))?;
    mutation(&mut catalog)?;
    Ok((
        catalog.config().sources.clone(),
        catalog.config().default_source.clone(),
    ))
}
```

For Add and Update, feed the result back to the form:

```rust
let result = mutate_catalog(&paths, |catalog| catalog.add(source));
match result {
    Ok((sources, default_source)) => state.mutation_succeeded(sources, default_source),
    Err(error) => state.mutation_failed(error.to_string()),
}
```

Use the same result tuple for Delete and MakeDefault, but call `update_sources` and `set_status` because those modes already returned to the list.

- [ ] **Step 5: Run wizard CRUD and lifecycle tests**

Run: `cargo test --test tui_render wizard_ -- --nocapture`

Expected: all wizard tests pass and no failed add or update clears the form.

- [ ] **Step 6: Commit the mutation lifecycle**

```bash
git add src/tui/wizard.rs src/commands/wizard.rs tests/tui_render.rs
git commit -m "Keep source forms open after save errors"
```

### Task 3: Render visible focus and contextual controls

**Files:**
- Modify: `src/tui/wizard.rs:430-620`
- Modify: `src/tui/view.rs:1-490`
- Modify: `src/i18n.rs:80-140,290-345,550-620`
- Test: `tests/tui_render.rs`

**Interfaces:**
- Consumes: `Frame::set_cursor_position`, `Line::width`, `WizardMode`, `MainView`, `FocusPane`, and `MessageKey`.
- Produces: mode-specific wizard footer keys, view-specific main footer keys, visible `›` focus markers, and responsive render branches that never hide the focused pane.

- [ ] **Step 1: Add failing cursor, footer, and responsive-focus tests**

Add a render helper that returns text and backend cursor state:

```rust
fn draw_wizard_with_cursor(
    state: &WizardState,
    width: u16,
    height: u16,
) -> (String, bool, ratatui::layout::Position) {
    let backend = TestBackend::new(width, height);
    let mut terminal = Terminal::new(backend).expect("terminal");
    terminal
        .draw(|frame| cc_switchy::tui::wizard::render(frame, state))
        .expect("draw wizard");
    let text = (0..height)
        .map(|y| {
            (0..width)
                .map(|x| terminal.backend().buffer()[(x, y)].symbol())
                .collect::<String>()
        })
        .collect::<Vec<_>>()
        .join("\n");
    (
        text,
        terminal.backend().cursor_visible(),
        terminal.backend().cursor_position(),
    )
}
```

Add assertions:

```rust
#[test]
fn wizard_empty_field_has_a_visible_marker_and_cursor() {
    let mut state = WizardState::new(Language::EnUs, Vec::new(), None);
    state.update(WizardAction::Add);
    state.update(WizardAction::Confirm);

    let (rendered, cursor_visible, cursor) = draw_wizard_with_cursor(&state, 100, 30);

    assert!(rendered.contains("› Name"));
    assert!(cursor_visible);
    assert!(cursor.x > 14);
    assert_eq!(cursor.y, 4);
}

#[test]
fn wizard_footer_matches_the_current_mode() {
    let list = WizardState::new(Language::EnUs, Vec::new(), None);
    assert!(draw_wizard(&list, 100, 30).contains("a add"));

    let mut form = WizardState::new(Language::ZhCn, Vec::new(), None);
    form.update(WizardAction::Add);
    form.update(WizardAction::Confirm);
    let rendered = draw_wizard(&form, 100, 30);
    assert!(rendered.contains("输入"));
    assert!(!rendered.contains("a 添加"));
}

#[test]
fn responsive_views_always_render_the_focused_pane() {
    let mut app = app(Language::EnUs, true);
    app.focus = FocusPane::Details;
    let providers = draw(&app, 100, 30);
    assert!(providers.contains("› Details"));

    app.view = MainView::Skills;
    app.focus = FocusPane::Agents;
    let agents = draw(&app, 70, 24);
    assert!(agents.contains("› Agents"));

    app.focus = FocusPane::List;
    let skills = draw(&app, 70, 24);
    assert!(skills.contains("› Skills"));
}
```

- [ ] **Step 2: Run the render tests and verify they fail**

Run:

```bash
cargo test --test tui_render wizard_empty_field_has_a_visible_marker_and_cursor wizard_footer_matches_the_current_mode responsive_views_always_render_the_focused_pane -- --nocapture
```

Expected: marker and contextual text assertions fail, the cursor is hidden, and medium/narrow layouts omit the focused pane.

- [ ] **Step 3: Add contextual message keys and translations**

Replace the generic footer keys with explicit keys:

```rust
TuiFooterProviders,
TuiFooterSkills,
TuiFooterActivity,
TuiFooterSources,
WizardFooterList,
WizardFooterForm,
WizardFooterNavigate,
WizardFooterBack,
```

English strings:

```text
Providers: ↑↓ move  Tab focus  [ ] Agent  Enter apply  s sync  w wizard  L language  q quit
Skills: ↑↓ move  Tab focus  [ ] Agent  s sync  w wizard  L language  q quit
Activity: s sync  w wizard  L language  q quit
Sources: ↑↓ move  Tab focus  s sync  t test  m default  w wizard  L language  q quit
List: a add  e edit  Enter details  x delete  t test  m default  L language  q exit
Form: type to edit  Tab/Shift+Tab field  Enter next/save  Esc discard  Ctrl+C exit
Navigate: ↑↓ choose  Enter confirm  Esc back  q exit
Back: Enter/Esc back  q exit
```

Add equivalent Simplified Chinese strings with the same keys and actions.

- [ ] **Step 4: Render the active form row and terminal cursor**

Build each form row with an explicit marker and style the whole line:

```rust
let active = index == state.field;
let marker = if active { "› " } else { "  " };
let line = Line::from(vec![
    Span::styled(marker, if active { active_style } else { Style::default() }),
    Span::styled(
        format!("{:14}", translator.text(field.label, &args)),
        if active { active_style } else { Style::default().fg(Color::DarkGray) },
    ),
    Span::styled(value.clone(), if active { active_style } else { Style::default() }),
]);
```

After rendering the list, place the cursor using terminal-cell width:

```rust
let displayed = displayed_form_value(&state.fields[state.field]);
let cursor_x = area
    .x
    .saturating_add(1)
    .saturating_add(2)
    .saturating_add(14)
    .saturating_add(Line::from(displayed).width() as u16)
    .min(area.right().saturating_sub(2));
let cursor_y = area
    .y
    .saturating_add(1)
    .saturating_add(state.field as u16)
    .min(area.bottom().saturating_sub(2));
frame.set_cursor_position((cursor_x, cursor_y));
```

Use one helper for both row text and cursor width so raw secrets never enter the render buffer:

```rust
fn displayed_form_value(field: &FormField) -> String {
    if field.label == MessageKey::FieldAccessKeyId {
        mask_access_id(&field.value)
    } else if field.secret {
        mask(&field.value)
    } else {
        field.value.clone()
    }
}
```

- [ ] **Step 5: Make pane focus visible and responsive branches focus-aware**

Change pane titles to include a text marker:

```rust
fn pane_block(title: &str, focused: bool) -> Block<'_> {
    let marker = if focused { "› " } else { "" };
    Block::default()
        .borders(Borders::ALL)
        .title(format!(" {marker}{title} "))
        .border_style(if focused {
            Style::default().fg(ACCENT)
        } else {
            Style::default().fg(MUTED)
        })
}
```

At medium Provider widths, render Agents plus Details when Details owns focus; otherwise render Agents plus the provider list. Split `render_skills` into layout selection plus a `render_skill_list` helper, and at narrow widths render Agents when `FocusPane::Agents` owns focus, otherwise render Skills.

- [ ] **Step 6: Select footer text by wizard mode and main view**

In the wizard renderer:

```rust
let footer_key = match state.mode {
    WizardMode::List => MessageKey::WizardFooterList,
    WizardMode::EditWebDav | WizardMode::EditS3 => MessageKey::WizardFooterForm,
    WizardMode::Details | WizardMode::TestConnection => MessageKey::WizardFooterBack,
    WizardMode::ChooseType
    | WizardMode::ConfirmDelete
    | WizardMode::ChooseReplacementDefault
    | WizardMode::LanguageSelect => MessageKey::WizardFooterNavigate,
};
```

In the main TUI renderer, select `TuiFooterProviders`, `TuiFooterSkills`, `TuiFooterActivity`, or `TuiFooterSources`. Append `r retry` or `r 重试` to Activity only when `app.progress.retry_available` is true.

- [ ] **Step 7: Run the complete TUI test target**

Run: `cargo test --test tui_render -- --nocapture`

Expected: all old and new tests pass, cursor tests report visible only in forms, and no secret fixture appears in rendered output.

- [ ] **Step 8: Commit focus and help changes**

```bash
git add src/tui/wizard.rs src/tui/view.rs src/i18n.rs tests/tui_render.rs
git commit -m "Keep TUI focus visible across views"
```

### Task 4: Update public documentation and package version

**Files:**
- Modify: `README.md:70-95,175-225`
- Modify: `Cargo.toml:1-6`
- Modify: `Cargo.lock:275-305`
- Test: `tests/readme_commands.rs`

**Interfaces:**
- Consumes: the implemented keyboard contract and current package version `0.1.0`.
- Produces: documented form input behavior and package version `0.1.1`.

- [ ] **Step 1: Update bilingual keyboard documentation**

State that printable characters are normal input inside forms, `Esc` discards the form, `q` exits outside forms, and `Ctrl+C` exits from every wizard screen. Keep the Chinese and English sections equivalent.

Change the Chinese private-CA version-specific statement from `v0.1.0` to `v0.1.1` because the limitation remains true for the new release.

- [ ] **Step 2: Bump the package version**

Change:

```toml
[package]
name = "cc-switchy"
version = "0.1.1"
```

After editing `Cargo.toml`, run `cargo check --locked`. Expected: Cargo reports that `Cargo.lock` needs to be updated. Then run `cargo check` to update the root package entry in `Cargo.lock` from `0.1.0` to `0.1.1` without changing dependency versions.

- [ ] **Step 3: Run documentation and version checks**

Run:

```bash
cargo test --test readme_commands
cargo metadata --no-deps --format-version 1
rg -n 'version = "0.1.0"|v0\.1\.0' Cargo.toml Cargo.lock README.md
```

Expected: README tests pass, metadata reports `cc-switchy 0.1.1`, and the final search returns no stale release-specific version in the package files or README.

- [ ] **Step 4: Commit the release version**

```bash
git add README.md Cargo.toml Cargo.lock
git commit -m "Prepare cc-switchy v0.1.1"
```

### Task 5: Verify, smoke test, tag, and push v0.1.1

**Files:**
- Verify: all tracked files
- Create Git tag: `v0.1.1`
- Push: `main` and `refs/tags/v0.1.1` to `origin`

**Interfaces:**
- Consumes: clean commits implementing Tasks 1 through 4.
- Produces: verified `main` and annotated tag `v0.1.1` on `git@github.com:ca-x/cc-switchy.git`.

- [ ] **Step 1: Run static and test verification**

Run:

```bash
cargo fmt --check
cargo clippy --all-targets --all-features -- -D warnings
cargo test --all-targets --all-features
cargo build --release --locked
git diff --check
git status --short --branch -uall
```

Expected: every command succeeds and the worktree is clean except that `main` is ahead of `origin/main` by the new commits.

- [ ] **Step 2: Run the PTY wizard smoke test**

Launch the release binary with an isolated home:

```bash
tmpdir=$(mktemp -d)
HOME="$tmpdir" target/release/cc-switchy --wizard
```

In the PTY:

1. Press `a`, then `Enter` to open the WebDAV form.
2. Type `qjkaextmwsL` and confirm every character remains in Name.
3. Press `Tab`, enter a URL, and verify the cursor moves to the end of the visible value.
4. Press `Esc` and verify the form returns to the list without exiting.
5. Reopen the form, enter valid fields, save, and verify the list shows the source.
6. Press `q` on the list and verify the process exits with the normal screen and cursor restored.

Remove the isolated temporary home after the process exits.

- [ ] **Step 3: Review release state and remote tag availability**

Run:

```bash
git fetch --tags origin
git log --oneline --decorate origin/main..HEAD
git tag --list v0.1.1
git ls-remote --tags origin refs/tags/v0.1.1
```

Expected: the commit list contains only the approved design, TUI repair, documentation, and version commits; both tag queries return no existing `v0.1.1`. If the remote tag exists, stop before tagging and select the next unused patch version consistently in Cargo and README.

- [ ] **Step 4: Create and verify the annotated tag**

```bash
git tag -a v0.1.1 -m "cc-switchy v0.1.1"
git show --stat --oneline v0.1.1
```

Expected: `v0.1.1` points at the verified release commit and includes the intended source, tests, README, Cargo version, design, and plan history.

- [ ] **Step 5: Push the branch and tag**

```bash
git push origin main
git push origin v0.1.1
```

Expected: both pushes succeed without force. Do not use `--force` or replace an existing tag.

- [ ] **Step 6: Confirm the remote release refs**

```bash
git status --short --branch -uall
git ls-remote origin refs/heads/main refs/tags/v0.1.1 refs/tags/v0.1.1^{}
```

Expected: local `main` matches `origin/main`; the tag and peeled tag object resolve to the verified release commit.
