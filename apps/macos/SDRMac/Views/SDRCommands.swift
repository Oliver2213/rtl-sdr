//
// SDRCommands.swift — menu-bar commands attached to the main
// scene via `.commands { ... }`.
//
// Replaces the default File > New (we don't have a document
// model) and adds Radio (start/stop, tune nudge), Bookmarks
// (add/show), and a View section that surfaces every activity-
// bar panel as a checkmarked toggle with its keyboard shortcut.

import SwiftUI

struct SDRCommands: Commands {
    let core: CoreModel

    var body: some Commands {
        CommandGroup(replacing: .newItem) {}

        CommandMenu("Radio") {
            Button("Start") { core.start() }
                .keyboardShortcut("r", modifiers: .command)
                .disabled(core.isRunning)
            Button("Stop") { core.stop() }
                .keyboardShortcut(".", modifiers: .command)
                .disabled(!core.isRunning)
            Divider()
            Button("Tune Up 100 kHz") {
                core.setCenter(core.centerFrequencyHz + 100_000)
            }
            .keyboardShortcut(.upArrow, modifiers: .command)
            Button("Tune Down 100 kHz") {
                // Clamp to 0 so repeated taps don't drive the
                // center frequency negative (the engine would
                // reject it but the UI would still show a
                // negative value in the toolbar).
                core.setCenter(max(core.centerFrequencyHz - 100_000, 0))
            }
            .keyboardShortcut(.downArrow, modifiers: .command)
            .disabled(core.centerFrequencyHz < 100_000)
        }

        CommandMenu("Bookmarks") {
            // Ellipsis: the command opens the Add Bookmark sheet
            // to collect a name/category before it executes.
            Button("Add Bookmark…") { core.bookmarkEditor = .add }
                .keyboardShortcut("d", modifiers: .command)
            Divider()
            // Reveal the Bookmarks panel (right activity bar
            // slot 2, ⌘⇧2). Same target the activity-bar icon
            // drives, surfaced here so the command is findable in
            // the menu bar too.
            Button("Show Bookmarks") {
                core.setSidebarRightSelected(RightActivity.bookmarks.rawValue)
                core.setSidebarRightOpen(true)
            }
        }

        // View menu — every activity-bar panel as a checkmarked
        // toggle with its shortcut. This is the authoritative home
        // for the ⌘1–6 / ⌘⇧1–2 accelerators (the activity-bar
        // icons are click accelerators, not shortcut owners, so
        // the shortcuts live here to avoid double registration).
        // `.sidebar` placement lands these in the system-provided
        // View menu next to Show/Hide Sidebar rather than minting
        // a second "View" menu.
        //
        // A Toggle renders as a checkmarked item: checked when the
        // panel is the selected AND open one on its side. Toggling
        // on selects + opens it (which un-checks the sibling
        // that was open); toggling off closes it while keeping the
        // selection — matching the activity-bar click semantics in
        // ActivityBarView.tap.
        CommandGroup(after: .sidebar) {
            Divider()
            ForEach(Array(LeftActivity.allCases)) { activity in
                Toggle(activity.label, isOn: leftPanelBinding(activity))
                    .keyboardShortcut(digitKey(activity.shortcutIndex),
                                      modifiers: .command)
            }
            Divider()
            ForEach(Array(RightActivity.allCases)) { activity in
                Toggle(activity.label, isOn: rightPanelBinding(activity))
                    .keyboardShortcut(digitKey(activity.shortcutIndex),
                                      modifiers: [.command, .shift])
            }
            Divider()
            // Toggle the left panel's visibility without changing
            // which activity is selected. Standard ⌃⌘S. (The old
            // NSSplitViewController.toggleSidebar selector was a
            // no-op here — this layout is a custom HStack, not an
            // NSSplitViewController — so drive the real state.)
            Button(core.sidebarLeftOpen ? "Hide Sidebar" : "Show Sidebar") {
                core.setSidebarLeftOpen(!core.sidebarLeftOpen)
            }
            .keyboardShortcut("s", modifiers: [.command, .control])
        }
    }

    // MARK: - Panel toggle bindings

    /// `isOn` binding for a left-column panel: checked when it is
    /// the selected + open left activity. Setting `true`
    /// selects + opens it; `false` closes the left panel while
    /// leaving the selection intact.
    private func leftPanelBinding(_ activity: LeftActivity) -> Binding<Bool> {
        Binding(
            get: {
                core.sidebarLeftSelected == activity.rawValue
                    && core.sidebarLeftOpen
            },
            set: { newValue in
                if newValue {
                    core.setSidebarLeftSelected(activity.rawValue)
                    core.setSidebarLeftOpen(true)
                } else {
                    core.setSidebarLeftOpen(false)
                }
            }
        )
    }

    /// Right-column counterpart of `leftPanelBinding`.
    private func rightPanelBinding(_ activity: RightActivity) -> Binding<Bool> {
        Binding(
            get: {
                core.sidebarRightSelected == activity.rawValue
                    && core.sidebarRightOpen
            },
            set: { newValue in
                if newValue {
                    core.setSidebarRightSelected(activity.rawValue)
                    core.setSidebarRightOpen(true)
                } else {
                    core.setSidebarRightOpen(false)
                }
            }
        )
    }

    /// Map a 1-based activity index to its number-row key. All
    /// current activities fall in 1...9; anything out of range
    /// falls back to "1" (unreachable given the enums today, but
    /// keeps the mapping total).
    private func digitKey(_ index: Int) -> KeyEquivalent {
        switch index {
        case 1: return "1"
        case 2: return "2"
        case 3: return "3"
        case 4: return "4"
        case 5: return "5"
        case 6: return "6"
        case 7: return "7"
        case 8: return "8"
        case 9: return "9"
        default: return "1"
        }
    }
}
