//
// ChannelCatalog.swift — named-channel catalogs used as scanner
// sources (the Scanner panel's "Scan sources" section).
//
// Mac-only data: standard fixed FCC / ITU channel allocations
// (FRS/GMRS, MURS, Marine VHF, NOAA Weather, CB, PMR446, and
// common amateur calling frequencies). Unlike the `bandPresets`
// slice — kept byte-for-byte in lockstep with the Linux
// `BAND_PRESETS` — these catalogs are a macOS-only surface, so
// there's no cross-frontend lockstep to preserve.
//
// An opted-in catalog's channels are projected into the scanner
// rotation by `CoreModel.refreshScannerChannels` and pushed over
// the existing scanner FFI — no new FFI, no config/schema change.
//
// Frequencies are receive (output) frequencies in North-America /
// ITU Region-2 conventions except PMR446 (Europe / Region 1,
// labelled as such). Repeater *input* frequencies are omitted —
// a receiver listens on the output.

import Foundation
import SdrCoreKit

/// One tunable named channel within a catalog.
struct RadioChannel: Identifiable, Hashable {
    /// Stable, catalog-unique id (channel names like "Ch 1"
    /// repeat across services, so the id is prefixed).
    let id: String

    /// Short channel name shown in the submenu ("Ch 1", "WX3",
    /// "22A", "2m FM Calling").
    let name: String

    let centerFrequencyHz: Double
    let demodMode: DemodMode
    let bandwidthHz: Double
}

/// A named group of channels — one submenu under "Channels".
struct ChannelCatalog: Identifiable, Hashable {
    var id: String { name }
    let name: String
    let channels: [RadioChannel]
}

// MARK: - Catalog data

/// All catalogs, in menu order. Built once; pure data.
let channelCatalogs: [ChannelCatalog] = [
    frsGmrsCatalog,
    mursCatalog,
    marineVhfCatalog,
    noaaWeatherCatalog,
    cbCatalog,
    pmr446Catalog,
    hamCallingCatalog,
]

// MARK: FRS / GMRS (US, FCC Part 95) — 22 shared channels

/// FRS and GMRS share the same 22 output frequencies (channels
/// 1–7 and 15–22 on 462 MHz, interstitial 8–14 on 467 MHz). NFM.
private let frsGmrsCatalog = ChannelCatalog(
    name: "FRS / GMRS",
    channels: [
        gmrs(1, 462_562_500), gmrs(2, 462_587_500), gmrs(3, 462_612_500),
        gmrs(4, 462_637_500), gmrs(5, 462_662_500), gmrs(6, 462_687_500),
        gmrs(7, 462_712_500),
        gmrs(8, 467_562_500), gmrs(9, 467_587_500), gmrs(10, 467_612_500),
        gmrs(11, 467_637_500), gmrs(12, 467_662_500), gmrs(13, 467_687_500),
        gmrs(14, 467_712_500),
        gmrs(15, 462_550_000), gmrs(16, 462_575_000), gmrs(17, 462_600_000),
        gmrs(18, 462_625_000), gmrs(19, 462_650_000), gmrs(20, 462_675_000),
        gmrs(21, 462_700_000), gmrs(22, 462_725_000),
    ]
)

private func gmrs(_ n: Int, _ hz: Double) -> RadioChannel {
    RadioChannel(id: "gmrs-\(n)", name: "Ch \(n)",
                 centerFrequencyHz: hz, demodMode: .nfm, bandwidthHz: 12_500)
}

// MARK: MURS (US, FCC Part 95J) — 5 channels

/// Channels 1–3 are 11.25 kHz narrowband on 151 MHz; 4 ("Blue
/// Dot") and 5 ("Green Dot") are 20 kHz on 154 MHz. NFM.
private let mursCatalog = ChannelCatalog(
    name: "MURS",
    channels: [
        RadioChannel(id: "murs-1", name: "Ch 1", centerFrequencyHz: 151_820_000,
                     demodMode: .nfm, bandwidthHz: 11_250),
        RadioChannel(id: "murs-2", name: "Ch 2", centerFrequencyHz: 151_880_000,
                     demodMode: .nfm, bandwidthHz: 11_250),
        RadioChannel(id: "murs-3", name: "Ch 3", centerFrequencyHz: 151_940_000,
                     demodMode: .nfm, bandwidthHz: 11_250),
        RadioChannel(id: "murs-4", name: "Ch 4 (Blue Dot)",
                     centerFrequencyHz: 154_570_000,
                     demodMode: .nfm, bandwidthHz: 20_000),
        RadioChannel(id: "murs-5", name: "Ch 5 (Green Dot)",
                     centerFrequencyHz: 154_600_000,
                     demodMode: .nfm, bandwidthHz: 20_000),
    ]
)

// MARK: Marine VHF (US) — common voice simplex channels

/// Curated set of commonly-monitored voice simplex channels
/// (where ship = coast frequency). Duplex-only and DSC-data
/// (Ch 70) channels are omitted. NFM.
private let marineVhfCatalog = ChannelCatalog(
    name: "Marine VHF",
    channels: [
        marine("06", 156_300_000), marine("09", 156_450_000),
        marine("10", 156_500_000), marine("13", 156_650_000),
        marine("16", 156_800_000), marine("22A", 157_100_000),
        marine("67", 156_375_000), marine("68", 156_425_000),
        marine("69", 156_475_000), marine("71", 156_575_000),
        marine("72", 156_625_000),
    ]
)

private func marine(_ ch: String, _ hz: Double) -> RadioChannel {
    RadioChannel(id: "marine-\(ch)", name: "Ch \(ch)",
                 centerFrequencyHz: hz, demodMode: .nfm, bandwidthHz: 16_000)
}

// MARK: NOAA Weather Radio (US) — WX1–WX7

private let noaaWeatherCatalog = ChannelCatalog(
    name: "NOAA Weather",
    channels: [
        wx(1, 162_550_000), wx(2, 162_400_000), wx(3, 162_475_000),
        wx(4, 162_425_000), wx(5, 162_450_000), wx(6, 162_500_000),
        wx(7, 162_525_000),
    ]
)

private func wx(_ n: Int, _ hz: Double) -> RadioChannel {
    RadioChannel(id: "wx-\(n)", name: "WX\(n)",
                 centerFrequencyHz: hz, demodMode: .nfm, bandwidthHz: 12_500)
}

// MARK: CB (US, FCC Part 95D) — 40 channels, AM

/// Standard 40-channel CB plan. Note the historical out-of-order
/// block: Ch 23 (27.255) sits above Ch 24/25 (27.235 / 27.245).
private let cbCatalog = ChannelCatalog(
    name: "CB",
    channels: [
        cb(1, 26_965_000), cb(2, 26_975_000), cb(3, 26_985_000),
        cb(4, 27_005_000), cb(5, 27_015_000), cb(6, 27_025_000),
        cb(7, 27_035_000), cb(8, 27_055_000), cb(9, 27_065_000),
        cb(10, 27_075_000), cb(11, 27_085_000), cb(12, 27_105_000),
        cb(13, 27_115_000), cb(14, 27_125_000), cb(15, 27_135_000),
        cb(16, 27_155_000), cb(17, 27_165_000), cb(18, 27_175_000),
        cb(19, 27_185_000), cb(20, 27_205_000), cb(21, 27_215_000),
        cb(22, 27_225_000), cb(23, 27_255_000), cb(24, 27_235_000),
        cb(25, 27_245_000), cb(26, 27_265_000), cb(27, 27_275_000),
        cb(28, 27_285_000), cb(29, 27_295_000), cb(30, 27_305_000),
        cb(31, 27_315_000), cb(32, 27_325_000), cb(33, 27_335_000),
        cb(34, 27_345_000), cb(35, 27_355_000), cb(36, 27_365_000),
        cb(37, 27_375_000), cb(38, 27_385_000), cb(39, 27_395_000),
        cb(40, 27_405_000),
    ]
)

private func cb(_ n: Int, _ hz: Double) -> RadioChannel {
    RadioChannel(id: "cb-\(n)", name: "Ch \(n)",
                 centerFrequencyHz: hz, demodMode: .am, bandwidthHz: 10_000)
}

// MARK: PMR446 (Europe / ITU Region 1) — 16 analogue channels

/// European licence-free UHF. 12.5 kHz spacing, NFM. Region-1
/// allocation — included for completeness and labelled EU.
private let pmr446Catalog = ChannelCatalog(
    name: "PMR446 (EU)",
    channels: [
        pmr(1, 446_006_250), pmr(2, 446_018_750), pmr(3, 446_031_250),
        pmr(4, 446_043_750), pmr(5, 446_056_250), pmr(6, 446_068_750),
        pmr(7, 446_081_250), pmr(8, 446_093_750), pmr(9, 446_106_250),
        pmr(10, 446_118_750), pmr(11, 446_131_250), pmr(12, 446_143_750),
        pmr(13, 446_156_250), pmr(14, 446_168_750), pmr(15, 446_181_250),
        pmr(16, 446_193_750),
    ]
)

private func pmr(_ n: Int, _ hz: Double) -> RadioChannel {
    RadioChannel(id: "pmr446-\(n)", name: "Ch \(n)",
                 centerFrequencyHz: hz, demodMode: .nfm, bandwidthHz: 12_500)
}

// MARK: Amateur calling frequencies (Region 2)

/// Common amateur simplex / calling frequencies. SSB calling
/// uses USB above 50 MHz per convention; FM calling uses NFM.
private let hamCallingCatalog = ChannelCatalog(
    name: "Ham Calling",
    channels: [
        RadioChannel(id: "ham-6m-ssb", name: "6m SSB Calling",
                     centerFrequencyHz: 50_125_000,
                     demodMode: .usb, bandwidthHz: 2_700),
        RadioChannel(id: "ham-6m-fm", name: "6m FM Calling",
                     centerFrequencyHz: 52_525_000,
                     demodMode: .nfm, bandwidthHz: 12_500),
        RadioChannel(id: "ham-2m-ssb", name: "2m SSB Calling",
                     centerFrequencyHz: 144_200_000,
                     demodMode: .usb, bandwidthHz: 2_700),
        RadioChannel(id: "ham-2m-fm", name: "2m FM Calling",
                     centerFrequencyHz: 146_520_000,
                     demodMode: .nfm, bandwidthHz: 12_500),
        RadioChannel(id: "ham-125m-fm", name: "1.25m FM Calling",
                     centerFrequencyHz: 223_500_000,
                     demodMode: .nfm, bandwidthHz: 12_500),
        RadioChannel(id: "ham-70cm-fm", name: "70cm FM Calling",
                     centerFrequencyHz: 446_000_000,
                     demodMode: .nfm, bandwidthHz: 12_500),
    ]
)
