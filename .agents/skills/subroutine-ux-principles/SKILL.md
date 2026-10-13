---
name: subroutine-ux-principles
description: Design and implement Subroutine UX using restrained, directly manipulable interfaces. Use for empty states, onboarding, creation, navigation, contextual controls, and changes to information density or interaction flow.
disable-model-invocation: false
---

# Subroutine UX Principles

Apply these principles when designing or modifying Subroutine. Treat layouts and individual view behavior as evolving decisions: inspect the current implementation and follow the user’s latest direction rather than preserving an old design specification.

## Show only what is useful now

Subroutine should reduce the amount of information the user has to process. Do not fill unused space with explanations merely because the screen looks sparse.

- Prefer useful structure, direct manipulation, and clear affordances over instructional copy.
- Keep onboarding restrained too. First use is not a license to add tutorials or repeated explanations.
- Avoid slogans, generic encouragement, and descriptions of obvious interface behavior.
- Use short labels for real controls. Put supporting context in concise tooltips and accessible names when needed.
- Explain actual failures, unavailable data, or consequential choices when that information helps the user act. Minimalism must not hide necessary information.
- Keep quiet states quiet; do not pressure the user to create more work simply because nothing needs attention.

## Design a nearly complete interaction

Aim for a prepared space with one obvious missing piece, rather than a pile of possible actions. The user should be able to complete that piece with a small, intuitive input.

- Use the actual working interface as the invitation to interact.
- Make missing pieces usable: a creation slot should become the content the user edits, not merely illustrate that possibility.
- Let spatial structure, handles, and interaction feedback communicate what can be done without explaining it.
- Do not substitute fake tasks, disabled sample content, or decorative skeletons for real interaction.
- Keep motion functional: preserve continuity rather than animating just to attract attention.
- Reuse the app’s visual language instead of introducing a separate treatment for each state.

## Prefer direct, in-place input

A creation affordance embedded in content should lead into editing that content locally, not send the user into an unrelated flow.

- Preserve the affordance’s footprint as it becomes editable content. Avoid gratuitous movement, resizing, recentering, or changes to surrounding content.
- Focus the relevant input so the user can begin typing immediately.
- Reserve separate creator flows for explicit user intent. Do not route every local creation action through a modal or dedicated creator.
- Keep any necessary choice small and local to the operation.
- Preserve input on validation or save failure. Cancellation should not leave accidental items behind.
- Keep provisional input distinct from saved data, and resolve its lifetime safely across navigation and workspace changes.

## Preserve continuity between states

Empty, drafting, and populated states should feel like the same interface—not separate screens that happen to share a name.

- Reuse normal layout primitives and content geometry.
- Show enough real structure to make the next interaction apparent, without adding filler content to make a view seem occupied.
- Do not let background refreshes replace unfinished input or steal focus.
- Preserve scroll position when beginning or cancelling an edit.
- If an entry point is meant to remain available, keep it independent of scrolling and filtering rather than forcing the user to find it in the content.
- When one object appears in several places, keep its previews synchronized while maintaining a single editing source.
- Extract components for meaningful shared responsibilities; do not fork rendering paths merely to handle an empty collection.

## Make controls predictable

- Keep a control’s meaning consistent. Selection may change the scope of an operation, but should not silently turn it into a different kind of action.
- Make the affected scope visible before committing, especially for operations spanning multiple objects or regions.
- Distinguish persistent entry points from hover-revealed controls. Visibility should follow the intended context, without unrelated controls appearing or remaining visible after that context ends.
- Avoid redundant competing affordances for the same immediate action.
- Keep contextual controls accessible by keyboard and provide meaningful accessible names even when visible labels are unnecessary.
- Use borders, emphasis, and other visual separators only when they communicate a useful relationship or state.

## Keep navigation predictable

- Preserve the user’s orientation and deliberate choices.
- Do not rearrange navigation or switch the current view simply because item counts change.
- Distinguish intentional navigation from temporary previews and incidental interactions when remembering user preferences.
- Improve the starting experience without creating an opaque “graduation” system that changes the interface underneath the user.

## Verification boundaries

**Leave visual testing to the user unless they explicitly authorize it.**

- Do not launch or focus the app, move the pointer, send keystrokes, drive GUI interactions, or capture screenshots/recordings without that authorization. An isolated test workspace does not make screen takeover acceptable.
- Apply this restriction to sub-agents and scripts too.
- Use code review, builds, and focused automated tests by default. Prefer a few reusable regressions for meaningful behavior over large test matrices or tests of trivial construction.
- Do not accumulate one-off verification scripts, compiled helpers, screenshots, logs, or temporary databases/settings. Avoid creating them unnecessarily, and clean up artifacts you created when they are no longer needed. Leave pre-existing user files alone.

## Review before finishing

- Is every visible element useful at this moment?
- Can the user discover the next action from the design rather than an explanation?
- Does the interaction complete a prepared piece of the interface instead of redirecting the user?
- Are control meaning, scope, and visibility predictable?
- Have focus, scrolling, unfinished input, and the user’s control of their screen been preserved?
