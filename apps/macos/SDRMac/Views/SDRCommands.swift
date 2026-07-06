//
// SDRCommands.swift — menu-bar commands attached to the main
// scene via `.commands { ... }`.
//
// Replaces the default File > New (we don't have a document model)
// and adds Radio (start/stop, tune nudge) and View menus.

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
            Button("Add Bookmark…") { core.showingAddBookmark = true }
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

        CommandGroup(after: .toolbar) {
            Button("Toggle Sidebar") {
                NSApp.keyWindow?.firstResponder?.tryToPerform(
                    #selector(NSSplitViewController.toggleSidebar(_:)),
                    with: nil
                )
            }
            .keyboardShortcut("s", modifiers: [.command, .control])
        }
    }
}
