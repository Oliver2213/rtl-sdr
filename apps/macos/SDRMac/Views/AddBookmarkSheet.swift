//
// AddBookmarkSheet.swift — modal sheet for saving the current
// tuning as a bookmark (#339 add path).
//
// The engine already snapshots the full tuning profile via
// `CoreModel.snapshotBookmark(name:)`; this sheet only collects
// the two user-facing labels — name and (optional) category —
// and shows a read-only summary of the tuning being captured so
// the user knows exactly what "Add" saves.
//
// Reached from three places, all flipping
// `CoreModel.showingAddBookmark`: the toolbar bookmark button,
// the `Bookmarks ▸ Add Bookmark…` menu command (⌘D), and the
// Bookmarks-panel header "+". On Add it writes through
// `BookmarksStore.add` and reveals the Bookmarks panel so the
// freshly-saved row is visible — matching the GTK behavior of
// landing the user back on a list that shows what they saved.

import SwiftUI
import SdrCoreKit

struct AddBookmarkSheet: View {
    @Environment(CoreModel.self) private var model
    @Environment(BookmarksStore.self) private var store
    @Environment(\.dismiss) private var dismiss

    /// Draft name. Seeded from the current frequency on appear;
    /// the user can overwrite it. An empty name falls back to
    /// the formatted frequency on save, matching the Linux
    /// quick-add's "blank name defaults to the frequency" rule.
    @State private var name: String = ""

    /// Optional free-text category. Unlike the Linux side — where
    /// categories only arrive via RadioReference import — a
    /// hand-typed category here groups the bookmark under a
    /// matching DisclosureGroup in the panel. Manual categories
    /// are a Mac parity-plus; empty stores `nil` (Uncategorized).
    @State private var category: String = ""

    /// Focus the name field on open so the user can immediately
    /// type or ⌘A-replace the seeded default.
    @FocusState private var nameFocused: Bool

    var body: some View {
        VStack(alignment: .leading, spacing: 0) {
            Text("Add Bookmark")
                .font(.headline)
                .padding(.horizontal, 20)
                .padding(.top, 20)
                .padding(.bottom, 4)

            Form {
                Section {
                    TextField("Name", text: $name)
                        .focused($nameFocused)
                    TextField("Category", text: $category,
                              prompt: Text("Optional"))
                }

                // Read-only summary of exactly what's captured.
                // The remaining tuning fields (squelch, gain, AGC,
                // de-emphasis) ride along automatically via
                // `snapshotBookmark` — these three are the ones a
                // user recognizes a station by, so they're the
                // ones we surface.
                Section("Tuning") {
                    LabeledContent("Frequency",
                                   value: formatRate(model.centerFrequencyHz))
                    LabeledContent("Mode", value: model.demodMode.label)
                    LabeledContent("Bandwidth",
                                   value: formatRate(model.bandwidthHz))
                }
            }
            .formStyle(.grouped)

            Divider()

            HStack {
                Spacer()
                Button("Cancel", role: .cancel) { dismiss() }
                    .keyboardShortcut(.cancelAction)
                Button("Add") { add() }
                    .keyboardShortcut(.defaultAction)
            }
            .padding(.horizontal, 20)
            .padding(.vertical, 16)
        }
        .frame(width: 380)
        .onAppear {
            if name.isEmpty {
                name = formatRate(model.centerFrequencyHz)
            }
            nameFocused = true
        }
    }

    private func add() {
        let trimmedName = name.trimmingCharacters(in: .whitespacesAndNewlines)
        let finalName = trimmedName.isEmpty
            ? formatRate(model.centerFrequencyHz)
            : trimmedName
        var bookmark = model.snapshotBookmark(name: finalName)
        let trimmedCategory = category
            .trimmingCharacters(in: .whitespacesAndNewlines)
        bookmark.rrCategory = trimmedCategory.isEmpty ? nil : trimmedCategory
        store.add(bookmark)
        // Reveal the Bookmarks panel so the new row is visible.
        model.setSidebarRightSelected(RightActivity.bookmarks.rawValue)
        model.setSidebarRightOpen(true)
        dismiss()
    }
}

#Preview {
    AddBookmarkSheet()
        .environment(CoreModel())
        .environment(BookmarksStore(
            storagePath: FileManager.default.temporaryDirectory
                .appendingPathComponent("preview-bookmarks.json")
        ))
}
