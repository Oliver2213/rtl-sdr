//
// BookmarkEditorSheet.swift — modal sheet for creating or
// editing a bookmark (#339).
//
// One sheet, two modes:
//   • `.add`  — save the current tuning. The engine snapshots
//     the full tuning profile via `CoreModel.snapshotBookmark`;
//     the sheet only collects Name + optional Category and shows
//     a read-only summary of the *current* tuning being captured.
//   • `.edit` — rename / recategorize an existing bookmark. The
//     tuning summary shows the bookmark's *saved* values (edit
//     doesn't recapture tuning — the "Update Tuning to Current"
//     row context-menu item does that).
//
// Both modes are driven by one `CoreModel.bookmarkEditor`
// presentation var: `.add` is set from the toolbar button, the
// Bookmarks ▸ Add Bookmark… command (⌘D), and the panel header
// "+"; `.edit(_)` is set from a row's "Edit…" context-menu item.

import SwiftUI
import SdrCoreKit

/// What the editor sheet operates on. `.edit` carries the
/// original bookmark so save can preserve its id and the tuning /
/// scanner fields the sheet doesn't surface.
///
/// Top-level (not nested in the view) and `Identifiable` so
/// `CoreModel` can hold a single `BookmarkEditorMode?` that
/// drives one `.sheet(item:)` — `nil` closed, `.add` for a new
/// bookmark, `.edit(_)` for an existing one.
enum BookmarkEditorMode: Identifiable, Equatable {
    case add
    case edit(Bookmark)

    var id: String {
        switch self {
        case .add: return "add"
        case .edit(let bookmark): return "edit-\(bookmark.id.uuidString)"
        }
    }
}

struct BookmarkEditorSheet: View {
    @Environment(CoreModel.self) private var model
    @Environment(BookmarksStore.self) private var store
    @Environment(\.dismiss) private var dismiss

    let mode: BookmarkEditorMode

    /// Draft name. Seeded on appear — from the current frequency
    /// (add) or the bookmark's name (edit). A blank name falls
    /// back to a sensible default on save rather than blocking it.
    @State private var name: String = ""

    /// Optional free-text category. Empty stores `nil`
    /// (Uncategorized). Hand-typed categories are a Mac
    /// parity-plus: the Linux frontend only gets categories via
    /// RadioReference import.
    @State private var category: String = ""

    /// Focus the name field on open so the user can immediately
    /// type or ⌘A-replace the seeded default.
    @FocusState private var nameFocused: Bool

    var body: some View {
        VStack(alignment: .leading, spacing: 0) {
            Text(isEditing ? "Edit Bookmark" : "Add Bookmark")
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

                // Read-only summary of the tuning tied to this
                // bookmark. On add these three come from the live
                // tuning (the rest — squelch, gain, AGC,
                // de-emphasis — ride along via `snapshotBookmark`);
                // on edit they're the bookmark's saved values.
                Section("Tuning") {
                    LabeledContent("Frequency", value: tuning.frequency)
                    LabeledContent("Mode", value: tuning.mode)
                    LabeledContent("Bandwidth", value: tuning.bandwidth)
                }
            }
            .formStyle(.grouped)

            Divider()

            HStack {
                Spacer()
                Button("Cancel", role: .cancel) { dismiss() }
                    .keyboardShortcut(.cancelAction)
                Button(isEditing ? "Save" : "Add") { commit() }
                    .keyboardShortcut(.defaultAction)
            }
            .padding(.horizontal, 20)
            .padding(.vertical, 16)
        }
        .frame(width: 380)
        .onAppear(perform: seed)
    }

    private var isEditing: Bool {
        if case .edit = mode { return true }
        return false
    }

    /// Frequency / Mode / Bandwidth display strings for the
    /// read-only summary, sourced per mode.
    private var tuning: (frequency: String, mode: String, bandwidth: String) {
        switch mode {
        case .add:
            return (formatRate(model.centerFrequencyHz),
                    model.demodMode.label,
                    formatRate(model.bandwidthHz))
        case .edit(let bm):
            return (bm.centerFrequencyHz.map(formatRate) ?? "—",
                    bm.demodMode?.label ?? "—",
                    bm.bandwidthHz.map(formatRate) ?? "—")
        }
    }

    private func seed() {
        switch mode {
        case .add:
            if name.isEmpty { name = formatRate(model.centerFrequencyHz) }
        case .edit(let bm):
            name = bm.name
            category = bm.rrCategory ?? ""
        }
        nameFocused = true
    }

    private func commit() {
        let trimmedName = name.trimmingCharacters(in: .whitespacesAndNewlines)
        let trimmedCategory = category
            .trimmingCharacters(in: .whitespacesAndNewlines)
        let finalCategory = trimmedCategory.isEmpty ? nil : trimmedCategory

        switch mode {
        case .add:
            let finalName = trimmedName.isEmpty
                ? formatRate(model.centerFrequencyHz)
                : trimmedName
            var bookmark = model.snapshotBookmark(name: finalName)
            bookmark.rrCategory = finalCategory
            store.add(bookmark)
            // Reveal the Bookmarks panel so the new row is visible.
            model.setSidebarRightSelected(RightActivity.bookmarks.rawValue)
            model.setSidebarRightOpen(true)
        case .edit(let bm):
            var updated = bm
            // Empty name keeps the bookmark's existing name rather
            // than blanking it.
            updated.name = trimmedName.isEmpty ? bm.name : trimmedName
            updated.rrCategory = finalCategory
            store.update(updated)
        }
        dismiss()
    }
}

#Preview("Add") {
    BookmarkEditorSheet(mode: .add)
        .environment(CoreModel())
        .environment(BookmarksStore(
            storagePath: FileManager.default.temporaryDirectory
                .appendingPathComponent("preview-bookmarks.json")
        ))
}
