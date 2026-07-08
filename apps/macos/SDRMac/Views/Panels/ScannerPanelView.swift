//
// ScannerPanelView.swift — Scanner activity panel (closes #447).
//
// Flat Sections in a grouped Form:
//
//   - Scanner      — master enable toggle
//   - Scan sources — which channels to sweep (bookmarks + the
//                    named channel catalogs)
//   - Active       — Channel / State rows + lockout button
//                    (button only visible when latched)
//   - Timing       — default dwell + default hang
//
// The Channel / State row text wraps multi-line by default —
// SwiftUI `Text` doesn't truncate without an explicit
// `.lineLimit(1)`, which we deliberately don't apply. Long
// bookmark names like "KY State Police District 7 Dispatch"
// stay fully readable in the sidebar.
//
// The scan set is projected in `CoreModel.refreshScannerChannels`:
// scan-enabled bookmarks (per-bookmark opt-in, #490) plus every
// channel of each opted-in catalog, union'd and pushed to the
// engine scanner over the existing FFI.

import SwiftUI
import SdrCoreKit

struct ScannerPanelView: View {
    var body: some View {
        Form {
            ScannerMasterSection()
            ScanSourcesSection()
            ActiveChannelSection()
            TimingSection()
        }
        .formStyle(.grouped)
    }
}

// ============================================================
//  Scanner master switch
// ============================================================

private struct ScannerMasterSection: View {
    @Environment(CoreModel.self) private var model

    var body: some View {
        Section {
            Toggle("Scanner", isOn: Binding(
                get: { model.scannerEnabled },
                set: { model.setScannerEnabled($0) }
            ))
        } header: {
            Text("Scanner")
        } footer: {
            Text("Sweep the selected sources, lingering on any channel with activity. Configure squelch on the Radio panel — the scanner treats an open squelch as activity.")
                .font(.caption)
        }
    }
}

// ============================================================
//  Scan sources — bookmarks + channel catalogs
// ============================================================

/// Which channels the scanner sweeps. A flat checklist: the
/// bookmark set plus one row per named channel catalog. Enabled
/// rows union into the rotation (`CoreModel.refreshScannerChannels`)
/// and re-push live via each setter, so toggling a source while
/// scanning updates the rotation without a restart.
private struct ScanSourcesSection: View {
    @Environment(CoreModel.self) private var model

    var body: some View {
        Section {
            Toggle("Bookmarks", isOn: Binding(
                get: { model.scanIncludeBookmarks },
                set: { model.setScanIncludeBookmarks($0) }
            ))
            ForEach(channelCatalogs) { catalog in
                Toggle(catalog.name, isOn: Binding(
                    get: { model.scanEnabledCatalogIDs.contains(catalog.id) },
                    set: { model.setScanCatalogEnabled(id: catalog.id, enabled: $0) }
                ))
            }
        } header: {
            Text("Scan sources")
        } footer: {
            // "Bookmarks" scans the ones opted in via the
            // Bookmarks panel's scan toggle. Leaving always-on
            // broadcasters like NOAA Weather off keeps the sweep
            // from holding on a channel that never goes quiet.
            Text("Channels to sweep. \"Bookmarks\" covers bookmarks with scanning enabled. Leave always-on broadcasters like NOAA Weather off so the scan doesn't stay parked on them.")
                .font(.caption)
        }
    }
}

// ============================================================
//  Active — current channel / state / lockout
// ============================================================

private struct ActiveChannelSection: View {
    @Environment(CoreModel.self) private var model

    var body: some View {
        Section {
            // Channel row. Latched: bookmark name + formatted
            // frequency. Idle: em-dash placeholder. Subtitle
            // wraps to multiple lines naturally — no
            // `.lineLimit(1)` so long names stay readable.
            //
            // `.textSelection(.enabled)` matches the pattern used
            // across the other Mac panels (TranscriptionPanel,
            // SourceSection, SettingsView) — long bookmark names
            // like "KY State Police District 7 Dispatch" are
            // selectable for paste into a search / log.
            LabeledContent("Channel") {
                Text(channelLabel)
                    .font(.callout)
                    .foregroundStyle(model.scannerActiveChannel == nil ? .secondary : .primary)
                    .multilineTextAlignment(.trailing)
                    .textSelection(.enabled)
            }

            // State row — tracks the engine's `ScannerState`
            // phase enum (Off / Retuning / Listening / Hang).
            LabeledContent("State") {
                Text(model.scannerState.label)
                    .font(.callout)
                    .foregroundStyle(.secondary)
            }

            // Lockout button — only visible when the scanner
            // has a channel latched. The button alone in an
            // always-visible row would leave a dangling labeled
            // strip when the scanner goes idle; the GTK panel
            // hides the whole row for the same reason.
            if model.scannerActiveChannel != nil {
                Button(role: .destructive) {
                    model.lockoutCurrentScannerChannel()
                } label: {
                    Label("Lockout this channel", systemImage: "nosign")
                }
            }
        } header: {
            Text("Active")
        } footer: {
            Text("Current channel and detector state. Lockout skips the active channel for the rest of the scanner session.")
                .font(.caption)
        }
    }

    /// Render the Channel row's right-side label. Latched:
    /// `"<bookmark name> — <freq MHz>"` to match the GTK
    /// panel's vocabulary. Idle: em-dash placeholder, same
    /// glyph the Linux side uses.
    private var channelLabel: String {
        guard let channel = model.scannerActiveChannel else {
            return "—"
        }
        let mhz = Double(channel.frequencyHz) / 1_000_000.0
        return String(format: "%@ — %.4f MHz", channel.name, mhz)
    }
}

// ============================================================
//  Timing — default dwell / hang
// ============================================================

private struct TimingSection: View {
    @Environment(CoreModel.self) private var model

    var body: some View {
        Section {
            LabeledContent("Default dwell") {
                Stepper(
                    value: Binding(
                        get: { model.scannerDefaultDwellMs },
                        set: { model.setScannerDefaultDwellMs($0) }
                    ),
                    in: CoreModel.scannerDwellMsRange,
                    step: 10
                ) {
                    Text("\(model.scannerDefaultDwellMs) ms")
                        .font(.callout)
                        .foregroundStyle(.secondary)
                        .monospacedDigit()
                }
            }
            LabeledContent("Default hang") {
                Stepper(
                    value: Binding(
                        get: { model.scannerDefaultHangMs },
                        set: { model.setScannerDefaultHangMs($0) }
                    ),
                    in: CoreModel.scannerHangMsRange,
                    step: 100
                ) {
                    Text("\(model.scannerDefaultHangMs) ms")
                        .font(.callout)
                        .foregroundStyle(.secondary)
                        .monospacedDigit()
                }
            }
        } header: {
            Text("Timing")
        } footer: {
            Text("How long the scanner lingers on each channel. Dwell is the settle window after retune; hang is the linger time after squelch closes before advancing.")
                .font(.caption)
        }
    }
}
