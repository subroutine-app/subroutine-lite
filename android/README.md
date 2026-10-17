# Android

JDK 17+, Android SDK Platform 36, NDK `29.0.14206865`, Rust, `cargo-ndk` 4.1.2, and Nushell. The parser is built from the same Rust sources as Desktop. Install Rust targets `aarch64-linux-android`, `x86_64-linux-android`, `armv7-linux-androideabi`, and `i686-linux-android`. See [native build details](native/README.md).

On macOS with Android Studio:

```sh
export JAVA_HOME="/Applications/Android Studio.app/Contents/jbr/Contents/Home"
export ANDROID_HOME="$HOME/Library/Android/sdk"
export PATH="$ANDROID_HOME/platform-tools:$PATH"
```

From the repo root, build/install/open on a connected device (`ANDROID_SERIAL` selects one):

```sh
./script/android-install
```

Build only, from `android/` (same directory for all Gradle commands below):

```sh
./gradlew --no-daemon :app:assembleDebug
```

Hosted sign-in/sync endpoints are configured by default. Sign-in is optional; local editing and reminders work without it. Bringing local items into an account requires an explicit merge. Remote push is [removed](../docs/PUSH.md).

## Local-only APK

Needs cached build dependencies:

```sh
./gradlew --offline --no-daemon -Pandroid.builder.sdkDownload=false \
  :app:assembleDebug -PsubroutineLite.offlineOnly=true
```

No Internet permission or remote auth/sync. Opens the local workspace; doesn't delete saved credentials, account caches or queued edits. Rebuilding without the flag can restore the account and resume sync.

## Local server

Start a local Lite server and Rauthy instance configured for the public Android client first. Replace `emulator-5554` with your device serial:

```sh
adb -s emulator-5554 reverse tcp:8080 tcp:8080
adb -s emulator-5554 reverse tcp:3300 tcp:3300

./gradlew --no-daemon :app:assembleDebug \
  -PrauthyIssuer=http://127.0.0.1:8080/auth/v1/ \
  -PrauthyClientId=subroutine-android \
  -PrauthyAllowInsecureLoopback=true \
  -PsubroutineLite.apiBaseUrl=http://127.0.0.1:3300/

adb -s emulator-5554 install -r app/build/outputs/apk/debug/app-debug.apk
adb -s emulator-5554 shell am start -n com.example.subroutine_lite/com.example.subroutine_simple.MainActivity
```

Override API, issuer and client ID together. Keep the exact issuer and trailing slash; don't substitute `localhost` or `10.0.2.2`. API URLs also need a trailing slash. Loopback HTTP is debug-only; release requires HTTPS, even offline-only. Omit the overrides to return to hosted defaults.

Provider client: public `subroutine-android`, code + S256 PKCE, scopes `openid subroutine:sync`, callback `dev.subroutine.lite://oauth/callback`.

Session credentials use Android Keystore encryption; sign-out doesn't remove account data or end browser SSO. Google uses hosted browser login; the full Android Google journey remains unverified. Build success isn't live auth validation.

No release signing configured. Use a separate Lite release key, not the full app's key or a debug key.

## Navigation and interaction

Primary tabs are **Home → Timeline → Upcoming → Browse**. Global Search and Calendar are in the toolbar; Backlog, Focus, Routines, Templates, and Settings & account remain in its overflow menu. Tab selection, timeline date/viewport, list state, and inline drafts survive navigation. No separate Queue view is ported.

- **Home:** the next eligible action and current/nearby event, with an inline action slot when empty. Free events remain visible but do not block the next action. The blocking window currently uses the desktop default of five minutes.
- **Upcoming:** Missed, Any time, Today, and future-date accordions with 20sp headings, animated item placement/fades, and a persistent expand/collapse-all control. Compact marker labels stay in date headers. Unscheduled queued actions remain reachable offline. Concrete future items are not capped at the marker expansion horizon.
- **Browse:** searchable collections for All, Unscheduled, Unqueued, Routines, and Saved. Unscheduled includes incomplete actions without a date/time, including queued any-time actions; Unqueued includes incomplete actions outside the queue, even when they have a due date. All combines these actions without duplicates, routine definitions, and saved action/event templates. Search matches titles, notes, and routine steps; query, filter, and scroll position survive tab changes and editor navigation. Action menus offer queue-any-time, save-for-reuse, duplicate, and confirmed deletion. Routines can be started when connected and pending edits have synchronized; saved items open the creator. Edit opens the existing detail editor, including removal and routine-step editing. Cached/local collections remain browsable and editable; routine starting remains online-only.
- **Cards:** bordered, 20dp-radius joined surfaces with full-width separators and consistent collapsed rows (80dp at normal font scale, increasing uniformly with accessibility text size). Long-press opens contextual operations; keyboard Menu/Shift+F10 and accessibility actions offer the same operations without a three-dot button. Notes are hidden until expanded. Backlog retains multiselect.
- **Calendar:** a compact month selector above the selected day's compound cards and inline creator. Overnight intervals appear on each occupied day, excluding an exact midnight end.
- **Timeline:** tap the background for an Action/Event/Marker/Signal menu at the nearest visible subdivision; hold the background to start an inline action. Inline titles use the shared parser and highlights, retain input on failure, and stay scoped to their workspace/dataset. Hold an item to move it; touch a top/bottom edge to resize timed actions/events and marker date spans. Resize grips appear centered on the active edge only during touch (or keyboard focus). Signals move without a duration; all-day actions move between dates. Dragging or resizing near the top/bottom scrolls continuously, with a snapped preview and haptic feedback on pickup, snapped time changes, release, and cancellation; release saves, while Back, cancellation, or a second pointer cancels. A refused save gives rejection feedback rather than success feedback. Keyboard/accessibility controls also step timing. Timed cards stay attached to physical time bounds and clip at the viewport; only their internal context sticks. Lane packing is independent of scrolling. All-day actions start as compact sticky count/title previews. Expand a preview to show its item cards in a bounded, scrollable panel; collapse it to recover timeline space. Expansion is remembered per date, and all-day actions never steal timed lanes. A dynamic outer-period button fits the surrounding hour, day, month, or year, with Desktop-style division labels. The compact toolbar puts outer-period fit, neutral Now, and zoom controls on one row; long-press Now resets zoom. Minor divisions use a nominal minimum of 64dp rather than physical pixels, so high-density phones no longer show cramped intervals.

Colors follow `../assets/themes/subroutine-{light,dark}.json`: warm paper/charcoal surfaces, teal selection, tangerine actions, sage completion, oxblood errors, and red current-time cues. Android maintains touch-sized controls rather than copying desktop hit areas.

Account initialization and uncached workspace loading show the Material 3 Expressive loading indicator. Reconnecting credentials are not presented as a sign-in failure before the initial authentication/cache attempts finish; actual failures remain actionable.

## Offline safety

Creation acknowledges the SQLite write before attempting network synchronization. Drafts retain a creation identity across retries, so a cancelled session or navigation change cannot create the same submission twice. Inline drafts are scoped by workspace, dataset, surface, and date, with activity-level saved state. The full creator overlay is available from every destination and retains raw input, selection, parsed metadata, and manual overrides across rotation and save failures.

Account edits use the durable outbox; foreground recovery and background work replay pending writes. **Sync now** in Settings requests recovery. Pull-to-resync deliberately refreshes canonical data without replaying writes. An expired session leaves verified cached account data editable; temporary authentication outages remain retryable. Conflicts require a choice, and import consent is revalidated against the account/session/dataset before merging. Fallback snapshots are isolated by server as well as account; legacy unscoped fallback files are not read, and existing SQLite workspaces are retained.

## Natural-language creator

The rounded-square **+** FAB opens a parser-backed creator in the existing window. Its 20dp radius is shared by the creator and compensated for the morph's unequal width/height scaling, keeping the visible corners circular. Item type is selected inside the creator; recognized dates, times, durations, and recurrence clauses have rounded highlights without changing the editable text. For example:

- `Call Ada tomorrow at 9am for 30m`
- `Review every Monday at 9am for 20m`
- `Stretch every 2 days for 3 occurrences`

Upcoming uses a compact **+** in each day header instead of idle creation cards. Tapping it reveals and focuses a draft immediately below that header; saving or cancelling removes the draft row again. Expanding all days therefore adds no empty creation rows. Active inline drafts retain the normal item-card layout. **More options** carries its raw title, selected date, and creation identity into the full creator with Details expanded. Saving clears only that originating draft; cancelling leaves it available inline.

Settings are ordered Account → Appearance → Notifications → Data (account is omitted in the local-only build).

Details can still be edited explicitly. A manual edit removes the corresponding recognized clause; removing a typed clause restores the manual/contextual value rather than leaving stale metadata behind. Only a verified parse of the current text can be submitted. Routine steps use the same parser for titles and durations. Recurrence is saved using the shared core schema, with the device's IANA timezone and any end/count bound.

The native parser runs off the UI thread and converts UTF-8 ranges into Android UTF-16 offsets. It is packaged for all four ABIs and runs locally, including in the no-Internet build. `buildParserJni` is part of normal Gradle builds; JVM tests build/load the actual host library through `buildParserHost`. Gradle `--offline` also puts Cargo offline.

The overlay requests focus when mounted, waits for the actual IME endpoint, and uses animated system insets to morph directly from the FAB to its keyboard-safe position. It does not run a bottom-sheet entrance followed by IME padding. Reversals start from the last rendered rectangle. Hardware/floating keyboards use a keyboard-free path; unanswered requests have bounded recovery. Android show/hide requests are best-effort, so arbitrarily late or cancelled IME responses are reconciled continuously rather than assumed to have finished. Logical visibility releases input and navigation immediately on close; exit animation retains only painting and has a bounded lifetime. Each new opening has a fresh creation session, including reopening during an exit. The FAB and overlay share the same visibility snapshot. Only the current creator's own submission locks its editing/dismissal controls; unrelated saves block submission but leave the draft editable and cancellable. Save completion uses the submitted session, and submission state is released in `finally`.

Headless Compose tests exercise actual drawing, touch dispatch, focus, repeated opening/closing, simulated IME insets, and unrelated busy operations. The reported device-only delayed-popup sequence is not reproduced by those tests; device verification is still needed. Debug builds emit `Creator` log entries for visibility, submission ownership, focus results, and IME/motion states, without titles or account data.

## Validation

```sh
./gradlew :app:assembleDebug :app:testDebugUnitTest :app:lintDebug
```

Tests cover durable replay and creation identity, import/account isolation, expiry/reminder policies, date and timeline boundaries, event availability, draft restoration, real JNI parsing, Unicode highlight offsets, manual/parsed field ownership, and IME-motion geometry. JVM tests do not establish browser sign-in, notification delivery, or rendered UI quality. Visual/device testing is intentionally separate.

## Remaining desktop parity

This is not yet full desktop feature parity:

- Recurrence can be authored in the natural-language creator, but local recurrence expansion is not implemented. Account completions queue offline; generating the next occurrence requires synchronization. Local-only recurring completion is refused explicitly.
- Starting routines and running the scheduling pipeline remain online-only. Routine definitions can be edited offline.
- Event availability is read and preserved, including when saving reusable templates; the dedicated busy-override editor is not ported. Using a template still prefills its basic fields rather than all recurrence/availability metadata.
- Timeline recurrence forecasting is not implemented; collision groups use an inspector rather than desktop mosaic packing. Zoom in to manipulate individual overlapping items.
- Live hosted sign-in/Google callbacks, notification delivery, visual/accessibility review, and release signing still need device/release validation.
